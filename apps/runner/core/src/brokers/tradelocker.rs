use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::{AccountSnapshot, AccountSummary, BrokerAdapter, OrderInstruction, OrderResult};

/// Port of server/routers/titan/adapters/tradelocker.py.
pub struct TradeLockerAdapter {
    email: String,
    password: String,
    server: String,
    base_url: String,
    session: Mutex<Option<(String, String)>>, // (access_token, account_id)
    accounts: Mutex<Vec<Value>>,              // raw all-accounts entries from the last login
}

impl TradeLockerAdapter {
    pub fn new(credentials: &HashMap<String, String>, is_paper: bool) -> Self {
        Self {
            email: credentials.get("email").cloned().unwrap_or_default(),
            password: credentials.get("password").cloned().unwrap_or_default(),
            server: credentials
                .get("server")
                .cloned()
                .unwrap_or_else(|| "OSPrime".to_string()),
            base_url: if is_paper {
                "https://demo.tradelocker.com/backend-api".to_string()
            } else {
                "https://live.tradelocker.com/backend-api".to_string()
            },
            session: Mutex::new(None),
            accounts: Mutex::new(Vec::new()),
        }
    }

    async fn login(&self) -> Result<(String, String), String> {
        let payload = json!({
            "email": self.email,
            "password": self.password,
            "server": self.server,
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(format!("{}/auth/jwt/token", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(resp.text().await.unwrap_or_default());
        }
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let access_token = data
            .get("accessToken")
            .and_then(|v| v.as_str())
            .ok_or("TradeLocker authentication failed")?
            .to_string();

        // The token response no longer embeds account info; fetch it separately
        // and prefer the account with the highest balance (demo accounts often
        // include an unfunded default alongside the funded one).
        let accounts_resp = client
            .get(format!("{}/auth/jwt/all-accounts", self.base_url))
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await
            .map_err(|e| format!("Could not fetch TradeLocker accounts: {e}"))?;
        if !accounts_resp.status().is_success() {
            let status = accounts_resp.status();
            let body = accounts_resp.text().await.unwrap_or_default();
            return Err(format!("Could not fetch TradeLocker accounts ({status}): {body}"));
        }
        let accounts_data: Value = accounts_resp.json().await.map_err(|e| e.to_string())?;
        let accounts = accounts_data
            .get("accounts")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        let balance_of = |v: &Value| -> f64 {
            v.get("accountBalance")
                .and_then(|b| b.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        let account = accounts
            .iter()
            .max_by(|a, b| balance_of(a).total_cmp(&balance_of(b)))
            .ok_or("TradeLocker account lookup returned no accounts")?;
        let account_id = account
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("TradeLocker account lookup missing id")?
            .to_string();

        *self.accounts.lock().unwrap() = accounts;
        *self.session.lock().unwrap() = Some((access_token.clone(), account_id.clone()));
        Ok((access_token, account_id))
    }

    async fn session(&self) -> Result<(String, String), String> {
        if let Some(s) = self.session.lock().unwrap().clone() {
            return Ok(s);
        }
        self.login().await
    }

    /// TradeLocker's /trade endpoints key off `accNum` (a small per-login
    /// index, e.g. "1"/"2"), not the `id` used in the URL path — they are
    /// different fields on the same all-accounts entry.
    fn acc_num_for(&self, account_id: &str) -> String {
        self.accounts
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id))
            .and_then(|a| a.get("accNum"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| account_id.to_string())
    }

    fn order_payload(order: &OrderInstruction) -> Value {
        let mut payload = Map::new();
        payload.insert("symbol".to_string(), json!(order.symbol));
        payload.insert(
            "orderType".to_string(),
            json!(match order.order_type.to_lowercase().as_str() {
                "limit" => "Limit",
                "stop" => "Stop",
                _ => "Market",
            }),
        );
        payload.insert(
            "side".to_string(),
            json!(if order.side.to_lowercase() == "sell" {
                "Sell"
            } else {
                "Buy"
            }),
        );
        payload.insert("quantity".to_string(), json!(order.quantity));
        payload.insert("timeInForce".to_string(), json!("GTC"));
        if let Some(limit_price) = order.limit_price {
            payload.insert("limitPrice".to_string(), json!(limit_price));
        }
        if let Some(stop_loss) = order.stop_loss {
            payload.insert("stopLoss".to_string(), json!({ "rate": stop_loss }));
        }
        if let Some(take_profit) = order.take_profit {
            payload.insert("takeProfit".to_string(), json!({ "rate": take_profit }));
        }
        Value::Object(payload)
    }
}

#[async_trait]
impl BrokerAdapter for TradeLockerAdapter {
    async fn authenticate(&self) -> Result<AccountSnapshot, String> {
        let (_, account_id) = self.login().await?;
        Ok(AccountSnapshot {
            account_id: Some(account_id),
            balance: 0.0,
            buying_power: 0.0,
        })
    }

    async fn get_account(&self) -> Result<AccountSnapshot, String> {
        let (token, account_id) = self.session().await?;
        let acc_num = self.acc_num_for(&account_id);
        let resp = reqwest::Client::new()
            .get(format!("{}/trade/accounts/{}", self.base_url, account_id))
            .header("Authorization", format!("Bearer {token}"))
            .header("accNum", &acc_num)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let data: serde_json::Value = resp.json().await.unwrap_or_default();
        let account = data.get("account").unwrap_or(&data);
        Ok(AccountSnapshot {
            account_id: Some(account_id),
            balance: super::json_get(account, "balance").unwrap_or(0.0),
            buying_power: super::json_get(account, "availableMargin").unwrap_or(0.0),
        })
    }

    async fn submit_order(&self, order: &OrderInstruction) -> OrderResult {
        let (token, account_id) = match self.session().await {
            Ok(s) => s,
            Err(e) => {
                return OrderResult {
                    success: false,
                    status: "rejected".to_string(),
                    error: Some(e),
                    ..Default::default()
                }
            }
        };
        let acc_num = self.acc_num_for(&account_id);
        let payload = Self::order_payload(order);
        let resp = reqwest::Client::new()
            .post(format!(
                "{}/trade/accounts/{}/orders",
                self.base_url, account_id
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("accNum", &acc_num)
            .json(&payload)
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let data: serde_json::Value = r.json().await.unwrap_or_default();
                OrderResult {
                    success: true,
                    broker_order_id: data
                        .get("id")
                        .or_else(|| data.get("orderId"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    status: "pending_new".to_string(),
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

    async fn list_accounts(&self) -> Vec<AccountSummary> {
        if self.accounts.lock().unwrap().is_empty() {
            let _ = self.session().await;
        }
        self.accounts
            .lock()
            .unwrap()
            .iter()
            .filter_map(|a| {
                Some(AccountSummary {
                    id: a.get("id")?.as_str()?.to_string(),
                    acc_num: a.get("accNum")?.as_str()?.to_string(),
                    balance: a
                        .get("accountBalance")
                        .and_then(|b| b.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0),
                    currency: a
                        .get("currency")
                        .and_then(|v| v.as_str())
                        .unwrap_or("USD")
                        .to_string(),
                })
            })
            .collect()
    }

    fn set_active_account(&self, account_id: &str) -> Result<(), String> {
        let known = self
            .accounts
            .lock()
            .unwrap()
            .iter()
            .any(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id));
        if !known {
            return Err(format!("Unknown TradeLocker account id: {account_id}"));
        }
        let mut session = self.session.lock().unwrap();
        let Some((token, _)) = session.clone() else {
            return Err("not logged in yet".to_string());
        };
        *session = Some((token, account_id.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_order() -> OrderInstruction {
        OrderInstruction {
            client_order_id: None,
            symbol: "XAUUSD".to_string(),
            side: "sell".to_string(),
            quantity: 0.1,
            order_type: "market".to_string(),
            limit_price: None,
            stop_loss: None,
            take_profit: None,
        }
    }

    #[test]
    fn market_payload_omits_empty_optional_fields() {
        let payload = TradeLockerAdapter::order_payload(&base_order());

        assert_eq!(payload["symbol"], "XAUUSD");
        assert_eq!(payload["orderType"], "Market");
        assert_eq!(payload["side"], "Sell");
        assert!(payload.get("limitPrice").is_none());
        assert!(payload.get("stopLoss").is_none());
        assert!(payload.get("takeProfit").is_none());
    }

    #[test]
    fn protected_payload_includes_trade_locker_exit_fields() {
        let mut order = base_order();
        order.stop_loss = Some(2395.5);
        order.take_profit = Some(2368.25);

        let payload = TradeLockerAdapter::order_payload(&order);

        assert_eq!(payload["stopLoss"]["rate"], 2395.5);
        assert_eq!(payload["takeProfit"]["rate"], 2368.25);
    }
}
