use serde::{Deserialize, Serialize};

/// Everything here lives only on whatever machine runs the runner — a
/// subscriber's laptop (desktop app) or a small cloud instance they own
/// (headless binary). Nothing in this struct is ever sent to Ellipsys except
/// `portal_base_url`/`runner_token`, which authenticate the polling loop —
/// never the broker credentials.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunnerConfig {
    pub portal_base_url: String,
    pub runner_token: Option<String>,
    pub venue: Option<String>,
    pub is_paper: bool,
    /// Broker login fields, keyed per-broker (e.g. "api_key", "api_secret",
    /// "email", "password", "server"). Never transmitted anywhere except
    /// directly to the broker's own API when submitting an order.
    pub credentials: std::collections::HashMap<String, String>,
}
