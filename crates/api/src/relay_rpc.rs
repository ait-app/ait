//! Authenticated control of this daemon's outbound online service connector.

use model::{Context, DispatchError, ErrorCode};
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
/// Returns `NotImplemented` for another method, or `Delivery` if outbound delivery fails.
pub(super) async fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    let Some(context) = context.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "relay.status.request" | "relay.start.request" | "relay.stop.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = execute(&context.request, state).await;
    context.respond(result).map_err(Into::into)
}

async fn execute(request: &model::Request, state: &Shared) -> Result<Value, ErrorCode> {
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
    }))
}
