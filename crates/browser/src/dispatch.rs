//! Browser request dispatch; callbacks are owned by the physical connection.
use crate::{broker::Broker, connection::Connection};
use model::{Context, DispatchError, ErrorCode};
/// Host-composed automation broker.
#[derive(Debug)]
pub struct State {
    /// Shared broker, absent in capability-limited hosts.
    pub broker: Option<Broker>,
}
/// Register a host using one connection subscription slot.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business errors are delivered in the response.
pub fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    let Some(mut context) = context.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "browser.host.register.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

    let result = if context.runtime.cancellation.is_cancelled() {
        Err(ErrorCode::ServerDraining)
    } else if context.available_subscriptions == 0 {
        Err(ErrorCode::ResourceExhausted)
    } else if let Some(broker) = &state.broker {
        connection.register(
            broker,
            std::mem::take(&mut context.request.params),
            context.outbound.clone(),
        )
    } else {
        Err(ErrorCode::UnsupportedCapability)
    };
    context.respond(result).map_err(Into::into)
}
