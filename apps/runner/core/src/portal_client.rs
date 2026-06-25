use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::brokers::AccountSummary;

#[derive(Debug, Clone, Deserialize)]
pub struct Instruction {
    pub id: String,
    #[serde(rename = "deploymentId")]
    pub deployment_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct InstructionsResponse {
    instructions: Vec<Instruction>,
}

pub struct PortalClient {
    base_url: String,
    runner_token: String,
}

impl PortalClient {
    pub fn new(base_url: &str, runner_token: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            runner_token: runner_token.to_string(),
        }
    }

    pub async fn fetch_instructions(&self) -> Result<Vec<Instruction>, String> {
        let resp = reqwest::Client::new()
            .get(format!("{}/api/portal/runner/instructions", self.base_url))
            .bearer_auth(&self.runner_token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(resp.text().await.unwrap_or_default());
        }
        let body: InstructionsResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(body.instructions)
    }

    pub async fn ack(
        &self,
        instruction_id: &str,
        status: &str,
        ack_result: serde_json::Value,
    ) -> Result<(), String> {
        let resp = reqwest::Client::new()
            .post(format!("{}/api/portal/runner/ack", self.base_url))
            .bearer_auth(&self.runner_token)
            .json(&json!({
                "instructionId": instruction_id,
                "status": status,
                "ackResult": ack_result,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(resp.text().await.unwrap_or_default());
        }
        Ok(())
    }

    pub async fn heartbeat(
        &self,
        venue: &str,
        status: &str,
        account_id: Option<&str>,
        is_paper: bool,
        balance_usd: Option<f64>,
        error_message: Option<&str>,
        accounts: &[AccountSummary],
    ) -> Result<HeartbeatResponse, String> {
        let resp = reqwest::Client::new()
            .post(format!("{}/api/portal/runner/heartbeat", self.base_url))
            .bearer_auth(&self.runner_token)
            .json(&json!({
                "venue": venue,
                "status": status,
                "accountId": account_id,
                "isPaper": is_paper,
                "balanceUsd": balance_usd,
                "errorMessage": error_message,
                "accounts": accounts,
                "runnerVersion": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(resp.text().await.unwrap_or_default());
        }
        Ok(resp.json().await.unwrap_or_default())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HeartbeatResponse {
    #[serde(rename = "preferredAccountId")]
    pub preferred_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RunnerStatus {
    pub connected: bool,
    pub last_poll_at: Option<String>,
    pub last_error: Option<String>,
}
