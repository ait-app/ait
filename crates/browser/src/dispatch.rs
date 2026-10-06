//! Browser request dispatch; callbacks are owned by the physical connection.
use crate::{broker::Broker, capabilities::IMPLEMENTED_GROUPS, connection::Connection};
use model::{Context, ErrorCode, outbound::QueueError};
/// Host-composed automation broker.
#[derive(Debug)]
pub struct State {
    /// Shared broker, absent in capability-limited hosts.
    pub broker: Option<Broker>,
}
/// Register a host using one connection subscription slot.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns outbound queue failures after returning business errors in the response.
pub fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    let Some((_, mut context)) = Context::take_matching(context, IMPLEMENTED_GROUPS) else {
        return Ok(());
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
    context.respond(result)
}
