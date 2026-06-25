use std::collections::HashMap;
use std::sync::Arc;

use ellipsys_runner_core::{poller, RunnerConfig};
use tokio::sync::Mutex;

/// Credential field names used across the supported brokers. The headless
/// runner reads only the ones present as env vars (e.g. an Alpaca deploy
/// sets ELLIPSYS_CRED_API_KEY/ELLIPSYS_CRED_API_SECRET and leaves the rest
/// unset). These env vars live entirely in whatever cloud account the
/// subscriber deployed to (DigitalOcean App Platform, Railway, etc.) —
/// Ellipsys never sees them.
const CREDENTIAL_KEYS: &[&str] = &[
    "api_key",
    "api_secret",
    "client_id",
    "host",
    "port",
    "private_key",
    "email",
    "password",
    "server",
];

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn config_from_env() -> RunnerConfig {
    let mut credentials = HashMap::new();
    for key in CREDENTIAL_KEYS {
        let env_name = format!("ELLIPSYS_CRED_{}", key.to_uppercase());
        if let Some(value) = env_var(&env_name) {
            credentials.insert(key.to_string(), value);
        }
    }

    RunnerConfig {
        portal_base_url: env_var("PORTAL_BASE_URL")
            .unwrap_or_else(|| "https://ellipsys-app.vercel.app".to_string()),
        runner_token: env_var("RUNNER_TOKEN"),
        venue: env_var("VENUE"),
        is_paper: env_var("IS_PAPER")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true),
        credentials,
    }
}

#[tokio::main]
async fn main() {
    let config = config_from_env();

    if config.runner_token.is_none() {
        eprintln!("RUNNER_TOKEN is required — generate one from the Ellipsys portal's Broker page");
        std::process::exit(1);
    }
    if config.venue.is_none() {
        eprintln!("VENUE is required (alpaca, ibkr, kraken, or tradelocker)");
        std::process::exit(1);
    }

    println!(
        "Ellipsys Runner (headless) starting — venue={}, paper={}, portal={}",
        config.venue.as_deref().unwrap_or("?"),
        config.is_paper,
        config.portal_base_url
    );

    let status = Arc::new(Mutex::new(Default::default()));
    poller::run_loop(config, status).await;
}
