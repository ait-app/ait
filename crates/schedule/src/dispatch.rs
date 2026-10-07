//! Schedule request dispatch with bounded, connection-independent execution.
use crate::service::Schedules;
use model::{Context, DispatchError, ErrorCode};

/// Host-composed schedule service.
#[derive(Debug)]
pub struct State {
    /// Persistent scheduler, absent in capability-limited hosts.
    pub schedules: Option<Schedules>,
}
/// Execute schedule requests; run-once waits do not block subsequent connection messages.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business failures use the stable schedule RPC error code.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    let Some(mut context) = context.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "schedule.create.request"
                | "schedule.list.request"
                | "schedule.inspect.request"
                | "schedule.logs.request"
                | "schedule.update.request"
                | "schedule.pause.request"
                | "schedule.resume.request"
                | "schedule.delete.request"
                | "schedule.run_once.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

    let Some(schedules) = &state.schedules else {
        return context
            .respond(Err(ErrorCode::UnsupportedCapability))
            .map_err(Into::into);
    };
    let admission = context
        .runtime
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if context.runtime.cancellation.is_cancelled() {
        return context
            .respond(Err(ErrorCode::ServerDraining))
            .map_err(Into::into);
    }
    if context.request.method == "schedule.run_once.request" {
        let Ok(permit) = context.runtime.execution_waits.clone().try_acquire_owned() else {
            return context
                .respond(Err(ErrorCode::ResourceExhausted))
                .map_err(Into::into);
        };
        let schedules = schedules.clone();
        let outbound = context.outbound.clone();
        let failure = outbound.failure();
        let cancel = context.runtime.cancellation.clone();
        let request = context.request;
        context.runtime.tasks.spawn(async move {
            let _permit = permit;
            tokio::select! {
                () = cancel.cancelled() => {},
                () = failure.cancelled() => {},
                result = schedules.execute(&request.method, request.params) => {
                    let _ = outbound.respond(request.id, result.map_err(|_| ErrorCode::ScheduleRequestFailed));
                }
            }
        });
        drop(admission);
        return Ok(());
    }
    drop(admission);
    let params = std::mem::take(&mut context.request.params);
    let method = context.request.method.clone();
    let result = schedules.execute(&method, params).await;
    context
        .respond(result.map_err(|_| ErrorCode::ScheduleRequestFailed))
        .map_err(Into::into)
}
