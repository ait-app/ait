//! The retired Relay control methods. Clients, old ones included, still call
//! `relay.status.request` for the daemon's identity and management state, so
//! the methods stay negotiable; the control connection itself is gone and its
//! status is always offline.

use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode};
use serde_json::{Value, json};

use crate::{Connector, Status};

/// Relay request names and directions for host negotiation and envelope validation.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("relay.status.request"),
    MethodSpec::request("relay.start.request"),
    MethodSpec::request("relay.stop.request"),
];

/// Handle an admitted relay request. `relay.start.request` never starts a
/// connection: a managed host refuses it as `relay_managed`, any other as
/// `not_implemented`; `relay.stop.request` has nothing left to stop.
///
/// # Arguments
/// * `context` - Negotiated, authenticated request; retained when its method is not recognized.
/// * `connector` - The host-owned connector shared with other control transports.
/// * `management` - Optional non-secret host ownership status; managed hosts reject external control.
/// # Returns
/// Delivers the public connector status and its runtime identity once, consuming the request.
/// An already consumed context requires no further work.
/// # Errors
/// Returns `NotImplemented` for another method, or `Delivery` if outbound delivery fails.
pub fn request(
    context: &mut Option<Context<'_>>,
    connector: &Connector,
    management: Option<Value>,
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
    let result = execute(&context.request, connector, management);
    context.respond(result).map_err(Into::into)
}

fn execute(
    request: &model::Request,
    connector: &Connector,
    management: Option<Value>,
) -> Result<Value, ErrorCode> {
    if management
        .as_ref()
        .is_some_and(|status| status["mode"] == "managed")
        && matches!(
            request.method.as_str(),
            "relay.start.request" | "relay.stop.request"
        )
    {
        return Err(ErrorCode::RelayManaged);
    }
    match request.method.as_str() {
        "relay.start.request" => return Err(ErrorCode::NotImplemented),
        "relay.stop.request" | "relay.status.request" => {}
        _ => return Err(ErrorCode::MethodNotFound),
    }
    Ok(json!({
        "serverId": connector.local.server_id,
        "instanceId": connector.local.instance_id,
        "platform": std::env::consts::OS,
        "status": Status::default(),
        "management": management.unwrap_or_else(|| json!({"mode":"external"})),
    }))
}

#[cfg(test)]
mod tests;
