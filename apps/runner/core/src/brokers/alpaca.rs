use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Map, Value};

use super::{AccountSnapshot, BrokerAdapter, OrderInstruction, OrderResult};

/// Port of server/routers/titan/adapters/alpaca.py — same REST surface,
/// executed from the subscriber's own machine with credentials they hold.
pub struct AlpacaAdapter {
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl AlpacaAdapter {
    pub fn new(credentials: &HashMap<String, String>, is_paper: bool) -> Self {
        Self {
            api_key: credentials.get("api_key").cloned().unwrap_or_default(),
            api_secret: credentials.get("api_secret").cloned().unwrap_or_default(),
            base_url: if is_paper {
                "https://paper-api.alpaca.markets".to_string()
            } else {
                "https://api.alpaca.markets".to_string()
            },
        }
    }

    fn client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    fn headers(&self) -> Result<HeaderMap, String> {
        let mut headers = HeaderMap::new();
        let api_key = HeaderValue::from_str(&self.api_key)
            .map_err(|e| format!("Invalid Alpaca api_key header: {e}"))?;
        let api_secret = HeaderValue::from_str(&self.api_secret)
            .map_err(|e| format!("Invalid Alpaca api_secret header: {e}"))?;
        headers.insert("APCA-API-KEY-ID", api_key);
        headers.insert("APCA-API-SECRET-KEY", api_secret);
        Ok(headers)
    }

    fn order_payload(order: &OrderInstruction) -> Value {
        let order_type = order.order_type.to_lowercase();
        let mut payload = Map::new();
        payload.insert("symbol".to_string(), json!(order.symbol.to_uppercase()));
        payload.insert("qty".to_string(), json!(order.quantity.to_string()));
        payload.insert("side".to_string(), json!(order.side.to_lowercase()));
        payload.insert("type".to_string(), json!(order_type));
        payload.insert("time_in_force".to_string(), json!("day"));

        if order_type == "limit" {
            if let Some(price) = order.limit_price {
                payload.insert("limit_price".to_string(), json!(price.to_string()));
            }
        }

        if order.stop_loss.is_some() || order.take_profit.is_some() {
            payload.insert("order_class".to_string(), json!("bracket"));
            if let Some(stop_price) = order.stop_loss {
                payload.insert(
                    "stop_loss".to_string(),
                    json!({ "stop_price": stop_price.to_string() }),
                );
            }
            if let Some(limit_price) = order.take_profit {
                payload.insert(
                    "take_profit".to_string(),
                    json!({ "limit_price": limit_price.to_string() }),
                );
            }
        }

        Value::Object(payload)
    }

    async fn account(&self) -> Result<serde_json::Value, String> {
        let resp = self
            .client()
            .get(format!("{}/v2/account", self.base_url))
            .headers(self.headers()?)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(resp.text().await.unwrap_or_default());
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl BrokerAdapter for AlpacaAdapter {
    async fn authenticate(&self) -> Result<AccountSnapshot, String> {
        if self.api_key.is_empty() || self.api_secret.is_empty() {
            return Err("Alpaca api_key and api_secret are required".to_string());
        }
        self.get_account().await
    }

    async fn get_account(&self) -> Result<AccountSnapshot, String> {
        let data = self.account().await?;
        Ok(AccountSnapshot {
            account_id: data.get("id").and_then(|v| v.as_str()).map(String::from),
            balance: super::json_get(&data, "equity").unwrap_or(0.0),
            buying_power: super::json_get(&data, "buying_power").unwrap_or(0.0),
        })
    }

    async fn submit_order(&self, order: &OrderInstruction) -> OrderResult {
        let headers = match self.headers() {
            Ok(headers) => headers,
            Err(e) => {
                return OrderResult {
                    success: false,
                    status: "rejected".to_string(),
                    error: Some(e),
                    ..Default::default()
                }
            }
        };
        let payload = Self::order_payload(order);
        let resp = self
            .client()
            .post(format!("{}/v2/orders", self.base_url))
            .headers(headers)
            .json(&payload)
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                OrderResult {
                    success: true,
                    broker_order_id: body.get("id").and_then(|v| v.as_str()).map(String::from),
                    status: body
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("open")
                        .to_string(),
                    error: None,
                }
            }
            Ok(r) => OrderResult {
                success: false,
                status: "rejected".to_string(),
                error: Some(r.text().await.unwrap_or_default()),
                ..Default::default()
            },
            Err(e) => OrderResult {
                success: false,
                status: "rejected".to_string(),
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market_order() -> OrderInstruction {
        OrderInstruction {
            symbol: "aapl".to_string(),
            side: "buy".to_string(),
            quantity: 1.0,
            order_type: "market".to_string(),
            limit_price: None,
            stop_loss: None,
            take_profit: None,
        }
    }

    #[test]
    fn market_payload_omits_null_limit_and_exit_fields() {
        let payload = AlpacaAdapter::order_payload(&market_order());

        assert_eq!(payload["symbol"], "AAPL");
        assert_eq!(payload["type"], "market");
        assert!(payload.get("limit_price").is_none());
        assert!(payload.get("stop_loss").is_none());
        assert!(payload.get("take_profit").is_none());
        assert!(payload.get("order_class").is_none());
    }

    #[test]
    fn protected_payload_uses_alpaca_bracket_order_fields() {
        let mut order = market_order();
        order.stop_loss = Some(185.25);
        order.take_profit = Some(194.75);

        let payload = AlpacaAdapter::order_payload(&order);

        assert_eq!(payload["order_class"], "bracket");
        assert_eq!(payload["stop_loss"]["stop_price"], "185.25");
        assert_eq!(payload["take_profit"]["limit_price"], "194.75");
    }
}
