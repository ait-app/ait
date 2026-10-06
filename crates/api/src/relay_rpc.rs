//! Authenticated control of this daemon's outbound online service connector.

use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::{Value, json};

use crate::Shared;

/// Extra transport-owned methods, independent of the pinned Paseo catalog.
pub(super) const METHODS: &[&str] = &[
    "relay.status.request",
    "relay.start.request",
    "relay.stop.request",
];

/// Handle a negotiated relay request using this daemon's fixed local destination.
///
/// Returns the non-secret connector status and runtime identity through `context`.
/// # Errors
/// Returns a queue error if the bounded outbound connection is no longer writable.
pub(super) async fn request(context: Context<'_>, state: &Shared) -> Result<(), QueueError> {
    let result = execute(&context.request, state).await;
    context.respond(result)
}

async fn execute(request: &model::Request, state: &Shared) -> Result<Value, ErrorCode> {
    if state.managed_relay
        && matches!(
            request.method.as_str(),
            "relay.start.request" | "relay.stop.request"
        )
    {
        return Err(ErrorCode::RelayManaged);
    }
    match request.method.as_str() {
        "relay.start.request" => {
            let grant = serde_json::from_value(request.params.clone())
                .map_err(|_| ErrorCode::InvalidMessage)?;
            state
                .relay
                .start(grant)
                .await
                .map_err(|error| match error {
                    relay::Error::InvalidGrant => ErrorCode::InvalidMessage,
                    relay::Error::Transport | relay::Error::Protocol => ErrorCode::AgentIo,
                })?;
        }
        "relay.stop.request" => state.relay.stop().await,
        "relay.status.request" => {}
        _ => return Err(ErrorCode::MethodNotFound),
    }
    Ok(json!({
        "serverId": state.info.server_id,
        "instanceId": state.info.instance_id,
        "platform": std::env::consts::OS,
        "status": state.relay.status().await,
        "management": state.managed_status.lock().map_or(Value::Null, |s| s.clone()),
    }))
}
