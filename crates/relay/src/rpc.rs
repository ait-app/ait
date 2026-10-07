//! Control of a fixed local relay connector through shared RPC contracts.

use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode};
use serde_json::{Value, json};

use crate::{Connector, Error};

/// Relay request names and directions for host negotiation and envelope validation.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("relay.status.request"),
    MethodSpec::request("relay.start.request"),
    MethodSpec::request("relay.stop.request"),
];

/// Handle an admitted relay request using the connector's fixed local destination.
///
/// # Arguments
/// * `context` - Negotiated, authenticated request; retained when its method is not recognized.
/// * `connector` - The host-owned connector shared with other control transports.
/// # Returns
/// Delivers the public connector status and its runtime identity once, consuming the request.
/// An already consumed context requires no further work.
/// # Errors
/// Returns `NotImplemented` for another method, or `Delivery` if outbound delivery fails.
pub async fn request(
    context: &mut Option<Context<'_>>,
    connector: &Connector,
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
    let result = execute(&context.request, connector).await;
    context.respond(result).map_err(Into::into)
}

async fn execute(request: &model::Request, connector: &Connector) -> Result<Value, ErrorCode> {
    match request.method.as_str() {
        "relay.start.request" => {
            let grant = serde_json::from_value(request.params.clone())
                .map_err(|_| ErrorCode::InvalidMessage)?;
            connector.start(grant).await.map_err(|error| match error {
                Error::InvalidGrant => ErrorCode::InvalidMessage,
                Error::Transport | Error::Protocol => ErrorCode::AgentIo,
            })?;
        }
        "relay.stop.request" => connector.stop().await,
        "relay.status.request" => {}
        _ => return Err(ErrorCode::MethodNotFound),
    }
    Ok(json!({
        "serverId": connector.local.server_id,
        "instanceId": connector.local.instance_id,
        "platform": std::env::consts::OS,
        "status": connector.status().await,
    }))
}

#[cfg(test)]
mod tests;
