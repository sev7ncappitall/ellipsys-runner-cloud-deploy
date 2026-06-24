use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{AccountSnapshot, BrokerAdapter, OrderInstruction, OrderResult};

const SIDECAR_SOURCE: &str = include_str!("ibkr_sidecar.py");
const SIDECAR_FILENAME: &str = "ellipsys-runner-ibkr-sidecar.py";

pub struct IbkrAdapter {
    credentials: HashMap<String, String>,
    is_paper: bool,
}

#[derive(Serialize)]
struct SidecarRequest<'a> {
    action: &'a str,
    is_paper: bool,
    credentials: &'a HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<&'a OrderInstruction>,
}

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    ok: bool,
    account_id: Option<String>,
    balance: Option<f64>,
    buying_power: Option<f64>,
    broker_order_id: Option<String>,
    status: Option<String>,
    error: Option<String>,
}

impl IbkrAdapter {
    pub fn new(credentials: &HashMap<String, String>, is_paper: bool) -> Self {
        Self {
            credentials: credentials.clone(),
            is_paper,
        }
    }

    fn ensure_sidecar_script() -> Result<PathBuf, String> {
        let path = env::temp_dir().join(SIDECAR_FILENAME);
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != SIDECAR_SOURCE {
            fs::write(&path, SIDECAR_SOURCE)
                .map_err(|e| format!("Could not write IBKR sidecar script: {e}"))?;
        }
        Ok(path)
    }

    async fn invoke_with_python(
        &self,
        python_bin: &str,
        request: &SidecarRequest<'_>,
    ) -> Result<SidecarResponse, String> {
        let script_path = Self::ensure_sidecar_script()?;
        let payload = serde_json::to_vec(request)
            .map_err(|e| format!("Could not serialize IBKR request: {e}"))?;

        let mut child = Command::new(python_bin)
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start {python_bin}: {e}"))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("Could not open stdin for {python_bin}"))?;
        stdin
            .write_all(&payload)
            .await
            .map_err(|e| format!("Could not send IBKR request to {python_bin}: {e}"))?;
        drop(stdin);

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| format!("Could not wait for {python_bin}: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stdout.is_empty() {
            let detail = if stderr.is_empty() {
                format!("{python_bin} exited without returning IBKR data")
            } else {
                stderr
            };
            return Err(detail);
        }

        let response: SidecarResponse = serde_json::from_str(&stdout)
            .map_err(|e| format!("Invalid IBKR sidecar response: {e}. Raw output: {stdout}"))?;

        if output.status.success() || response.ok {
            Ok(response)
        } else {
            Err(response
                .error
                .clone()
                .or_else(|| (!stderr.is_empty()).then_some(stderr))
                .unwrap_or_else(|| format!("IBKR sidecar exited with {}", output.status)))
        }
    }

    async fn invoke(
        &self,
        action: &str,
        order: Option<&OrderInstruction>,
    ) -> Result<SidecarResponse, String> {
        let request = SidecarRequest {
            action,
            is_paper: self.is_paper,
            credentials: &self.credentials,
            order,
        };

        let mut last_error = None;
        let mut candidates = Vec::new();
        if let Ok(custom) = env::var("ELLIPSYS_IBKR_PYTHON") {
            if !custom.trim().is_empty() {
                candidates.push(custom);
            }
        }
        candidates.push("python3".to_string());
        candidates.push("python".to_string());

        for candidate in candidates {
            match self.invoke_with_python(&candidate, &request).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    last_error = Some(err.clone());
                    if err.contains("Could not start") {
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            "No usable Python runtime was found for the IBKR sidecar".to_string()
        }))
    }

    fn account_from_response(response: SidecarResponse) -> Result<AccountSnapshot, String> {
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "IBKR sidecar returned an unknown error".to_string()));
        }

        Ok(AccountSnapshot {
            account_id: response.account_id,
            balance: response.balance.unwrap_or(0.0),
            buying_power: response.buying_power.unwrap_or(0.0),
        })
    }
}

#[async_trait]
impl BrokerAdapter for IbkrAdapter {
    async fn authenticate(&self) -> Result<AccountSnapshot, String> {
        Self::account_from_response(self.invoke("authenticate", None).await?)
    }

    async fn get_account(&self) -> Result<AccountSnapshot, String> {
        Self::account_from_response(self.invoke("get_account", None).await?)
    }

    async fn submit_order(&self, order: &OrderInstruction) -> OrderResult {
        match self.invoke("submit_order", Some(order)).await {
            Ok(response) => OrderResult {
                success: response.ok,
                broker_order_id: response.broker_order_id,
                status: response.status.unwrap_or_else(|| {
                    if response.ok {
                        "pending_new".to_string()
                    } else {
                        "rejected".to_string()
                    }
                }),
                error: response.error,
            },
            Err(error) => OrderResult {
                success: false,
                status: "rejected".to_string(),
                error: Some(error),
                ..Default::default()
            },
        }
    }
}
