use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};

use super::{AccountSnapshot, BrokerAdapter, OrderInstruction, OrderResult};

const BASE_URL: &str = "https://api.kraken.com";

/// Port of server/routers/titan/adapters/kraken.py's HMAC-SHA512 request
/// signing — executed locally so the API key/private key never reach Ellipsys.
pub struct KrakenAdapter {
    api_key: String,
    private_key: String,
}

impl KrakenAdapter {
    pub fn new(credentials: &HashMap<String, String>) -> Self {
        Self {
            api_key: credentials.get("api_key").cloned().unwrap_or_default(),
            private_key: credentials.get("private_key").cloned().unwrap_or_default(),
        }
    }

    fn sign(&self, urlpath: &str, postdata: &str, nonce: &str) -> Result<String, String> {
        let mut hasher = Sha256::new();
        hasher.update(nonce.as_bytes());
        hasher.update(postdata.as_bytes());
        let sha_digest = hasher.finalize();

        let key_bytes = general_purpose::STANDARD
            .decode(&self.private_key)
            .map_err(|e| e.to_string())?;
        let mut mac = Hmac::<Sha512>::new_from_slice(&key_bytes).map_err(|e| e.to_string())?;
        mac.update(urlpath.as_bytes());
        mac.update(&sha_digest);
        Ok(general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
    }

    async fn private_request(
        &self,
        urlpath: &str,
        mut data: HashMap<String, String>,
    ) -> Result<serde_json::Value, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis()
            .to_string();
        data.insert("nonce".to_string(), nonce.clone());

        let postdata = serde_urlencoded::to_string(&data).map_err(|e| e.to_string())?;
        let signature = self.sign(urlpath, &postdata, &nonce)?;

        let resp = reqwest::Client::new()
            .post(format!("{BASE_URL}{urlpath}"))
            .header("API-Key", &self.api_key)
            .header("API-Sign", signature)
            .form(&data)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl BrokerAdapter for KrakenAdapter {
    async fn authenticate(&self) -> Result<AccountSnapshot, String> {
        self.get_account().await
    }

    async fn get_account(&self) -> Result<AccountSnapshot, String> {
        let data = self
            .private_request("/0/private/Balance", HashMap::new())
            .await?;
        if let Some(errors) = data.get("error").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                return Err(format!("{errors:?}"));
            }
        }
        let result = data.get("result").cloned().unwrap_or_default();
        let balance = super::json_get(&result, "ZUSD").unwrap_or(0.0);
        Ok(AccountSnapshot {
            account_id: Some("kraken".to_string()),
            balance,
            buying_power: balance,
        })
    }

    async fn submit_order(&self, order: &OrderInstruction) -> OrderResult {
        let pair = match order.symbol.to_uppercase().as_str() {
            "BTCUSD" => "XBTUSD".to_string(),
            other => other.to_string(),
        };
        let mut payload = HashMap::new();
        payload.insert("pair".to_string(), pair);
        payload.insert("type".to_string(), order.side.to_lowercase());
        payload.insert(
            "ordertype".to_string(),
            match order.order_type.to_lowercase().as_str() {
                "limit" => "limit".to_string(),
                _ => "market".to_string(),
            },
        );
        payload.insert("volume".to_string(), order.quantity.to_string());
        if let Some(price) = order.limit_price {
            payload.insert("price".to_string(), price.to_string());
        }

        match self.private_request("/0/private/AddOrder", payload).await {
            Ok(data) => {
                if let Some(errors) = data.get("error").and_then(|e| e.as_array()) {
                    if !errors.is_empty() {
                        return OrderResult {
                            success: false,
                            status: "rejected".to_string(),
                            error: Some(format!("{errors:?}")),
                            ..Default::default()
                        };
                    }
                }
                let txid = data
                    .get("result")
                    .and_then(|r| r.get("txid"))
                    .and_then(|t| t.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(String::from);
                OrderResult {
                    success: true,
                    broker_order_id: txid,
                    status: "pending_new".to_string(),
                    error: None,
                }
            }
            Err(e) => OrderResult {
                success: false,
                status: "rejected".to_string(),
                error: Some(e),
                ..Default::default()
            },
        }
    }
}
