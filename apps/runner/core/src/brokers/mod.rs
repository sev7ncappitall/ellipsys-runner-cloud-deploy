pub mod alpaca;
pub mod ibkr;
pub mod kraken;
pub mod tradelocker;

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderInstruction {
    #[serde(rename = "clientOrderId", default)]
    pub client_order_id: Option<String>,
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    #[serde(rename = "orderType")]
    pub order_type: String,
    #[serde(rename = "limitPrice")]
    pub limit_price: Option<f64>,
    #[serde(rename = "stopLoss")]
    pub stop_loss: Option<f64>,
    #[serde(rename = "takeProfit")]
    pub take_profit: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct OrderResult {
    pub success: bool,
    pub broker_order_id: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AccountSnapshot {
    pub account_id: Option<String>,
    pub balance: f64,
    pub buying_power: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    #[serde(rename = "accNum")]
    pub acc_num: String,
    pub balance: f64,
    pub currency: String,
}

/// Mirrors server/routers/titan/adapters/base.py's VenueAdapter — same
/// contract, ported so the runner can talk to a subscriber's broker
/// directly using credentials that never leave this machine.
#[async_trait]
pub trait BrokerAdapter: Send + Sync {
    async fn authenticate(&self) -> Result<AccountSnapshot, String>;
    async fn get_account(&self) -> Result<AccountSnapshot, String>;
    async fn submit_order(&self, order: &OrderInstruction) -> OrderResult;

    /// All accounts available under this login, if the venue supports more
    /// than one per credential set (e.g. TradeLocker demo accounts). Empty
    /// for venues where the credentials map to exactly one account.
    async fn list_accounts(&self) -> Vec<AccountSummary> {
        Vec::new()
    }

    /// Switch which account subsequent get_account/submit_order calls target,
    /// without re-authenticating. Errors if the venue doesn't support
    /// multiple accounts or the id isn't one of list_accounts().
    fn set_active_account(&self, _account_id: &str) -> Result<(), String> {
        Err("this broker doesn't support selecting an account".to_string())
    }
}

pub fn build_adapter(
    venue: &str,
    credentials: &HashMap<String, String>,
    is_paper: bool,
) -> Result<Box<dyn BrokerAdapter>, String> {
    match venue {
        "alpaca" => Ok(Box::new(alpaca::AlpacaAdapter::new(credentials, is_paper))),
        "ibkr" => Ok(Box::new(ibkr::IbkrAdapter::new(credentials, is_paper))),
        "kraken" => Ok(Box::new(kraken::KrakenAdapter::new(credentials))),
        "tradelocker" => Ok(Box::new(tradelocker::TradeLockerAdapter::new(credentials, is_paper))),
        other => Err(format!(
            "{other} isn't supported by the runner yet (the MetaTrader bridge is still in development)"
        )),
    }
}

pub fn json_get(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|v| v.as_f64())
}
