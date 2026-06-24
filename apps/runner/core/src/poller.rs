use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::brokers::{build_adapter, OrderInstruction};
use crate::config::RunnerConfig;
use crate::portal_client::{PortalClient, RunnerStatus};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// The whole point of the runner: this loop holds the subscriber's broker
/// credentials in memory only, polls Ellipsys for instructions Prism/Titan
/// generated, executes them directly against the broker, and reports the
/// result back. Ellipsys never sees the credentials used here.
pub async fn run_loop(config: RunnerConfig, status: Arc<Mutex<RunnerStatus>>) {
    let Some(token) = config.runner_token.clone() else {
        let mut s = status.lock().await;
        s.connected = false;
        s.last_error = Some("No runner token configured".to_string());
        return;
    };
    let Some(venue) = config.venue.clone() else {
        let mut s = status.lock().await;
        s.connected = false;
        s.last_error = Some("No broker venue configured".to_string());
        return;
    };

    let client = PortalClient::new(&config.portal_base_url, &token);
    let adapter = match build_adapter(&venue, &config.credentials, config.is_paper) {
        Ok(a) => a,
        Err(e) => {
            let mut s = status.lock().await;
            s.connected = false;
            s.last_error = Some(e);
            return;
        }
    };

    if let Err(e) = adapter.authenticate().await {
        let _ = client
            .heartbeat(&venue, "error", None, config.is_paper, None, Some(&e))
            .await;
        let mut s = status.lock().await;
        s.connected = false;
        s.last_error = Some(e);
        return;
    }

    let mut last_heartbeat = std::time::Instant::now() - HEARTBEAT_INTERVAL;

    loop {
        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            let account = adapter.get_account().await;
            let (hb_status, account_id, balance, err) = match &account {
                Ok(snap) => (
                    "connected",
                    snap.account_id.clone(),
                    Some(snap.balance),
                    None,
                ),
                Err(e) => ("error", None, None, Some(e.clone())),
            };
            let _ = client
                .heartbeat(
                    &venue,
                    hb_status,
                    account_id.as_deref(),
                    config.is_paper,
                    balance,
                    err.as_deref(),
                )
                .await;
            last_heartbeat = std::time::Instant::now();

            let mut s = status.lock().await;
            s.connected = account.is_ok();
            s.last_error = err;
        }

        match client.fetch_instructions().await {
            Ok(instructions) => {
                for instruction in instructions {
                    handle_instruction(&*adapter, &client, &instruction).await;
                }
                let mut s = status.lock().await;
                s.last_poll_at = Some(chrono::Utc::now().to_rfc3339());
            }
            Err(e) => {
                let mut s = status.lock().await;
                s.last_error = Some(e);
            }
        }

        sleep(POLL_INTERVAL).await;
    }
}

async fn handle_instruction(
    adapter: &dyn crate::brokers::BrokerAdapter,
    client: &PortalClient,
    instruction: &crate::portal_client::Instruction,
) {
    match instruction.kind.as_str() {
        "place_order" => {
            let order: Result<OrderInstruction, _> =
                serde_json::from_value(instruction.payload.clone());
            let result = match order {
                Ok(o) => adapter.submit_order(&o).await,
                Err(e) => crate::brokers::OrderResult {
                    success: false,
                    status: "rejected".to_string(),
                    error: Some(format!("invalid order payload: {e}")),
                    ..Default::default()
                },
            };
            let ack_status = if result.success { "acked" } else { "failed" };
            let _ = client
                .ack(
                    &instruction.id,
                    ack_status,
                    json!({
                        "brokerOrderId": result.broker_order_id,
                        "status": result.status,
                        "error": result.error,
                    }),
                )
                .await;
        }
        "pause" | "resume" | "stop" => {
            // TODO: track a paused/stopped flag per deployment and skip
            // place_order while set. Today the portal's deployment status
            // already reflects pause/resume/stop; this ack just confirms
            // the runner saw it. Enforcing it locally is the next increment.
            let _ = client.ack(&instruction.id, "acked", json!({})).await;
        }
        other => {
            let _ = client
                .ack(
                    &instruction.id,
                    "failed",
                    json!({ "error": format!("unknown instruction kind: {other}") }),
                )
                .await;
        }
    }
}
