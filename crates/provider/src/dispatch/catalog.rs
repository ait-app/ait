//! Discovery replies are tracked independently of the physical WebSocket reader.

use model::outbound::QueueError;
use model::{Context, ErrorCode};

use super::State;

pub(super) fn request(context: Context<'_>, state: &State) -> Result<(), QueueError> {
    let admission = state
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.cancellation.is_cancelled() {
        return context.respond(Err(ErrorCode::ServerDraining));
    }
    let Some(execution) = &state.agent_execution else {
        return context.respond(Err(ErrorCode::UnsupportedCapability));
    };
    let mut response =
        match execution.admit_catalog(&context.request.method, context.request.params) {
            Ok(response) => response,
            Err(error) => {
                return context
                    .outbound
                    .respond(context.request.id, Err(error.into()));
            }
        };
    let outbound = context.outbound.clone();
    let request_id = context.request.id;
    // Admission happens before spawn, and the response owner retains its permit until
    // outbound delivery. Dropping the socket cannot cancel an accepted cache/refresh update.
    state.tasks.spawn(async move {
        let result = response.receive().await.map_err(Into::into);
        let _ = outbound.respond(request_id, result);
        drop(response);
    });
    drop(admission);
    Ok(())
}
