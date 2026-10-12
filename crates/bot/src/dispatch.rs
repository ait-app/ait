//! Ait request dispatch for the installed Bot service.

use model::{Context, DispatchError, ErrorCode};

use crate::service::Bots;

/// Host-installed Bot service.
#[derive(Debug, Default)]
pub struct State {
    /// Complete application service, absent in capability-limited hosts.
    pub bots: Option<Bots>,
}

/// Consume Bot requests and leave other component requests available for dispatch.
/// # Errors
/// Returns `NotImplemented` for other methods and `Delivery` for outbound failures.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = context.take_if(|context| {
        crate::capabilities::METHODS
            .iter()
            .any(|method| method.name == context.request.method)
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = if context.runtime.cancellation.is_cancelled() {
        Err(ErrorCode::ServerDraining)
    } else if let Some(bots) = &state.bots {
        let params = std::mem::take(&mut context.request.params);
        bots.execute(&context.request.method, params).await
    } else {
        Err(ErrorCode::UnsupportedCapability)
    };
    context.respond(result).map_err(Into::into)
}
