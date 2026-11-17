/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("agent.create.request"),
    MethodSpec::request("agent.resume.request"),
    MethodSpec::request("agent.message.send.request"),
    MethodSpec::request("agent.cancel.request"),
    MethodSpec::request("agent.finish.wait.request"),
    MethodSpec::request("agent.model.set.request"),
    MethodSpec::request("agent.thinking.set.request"),
    MethodSpec::request("agent.config.apply.request"),
    MethodSpec::request("provider.sessions.recent.list.request"),
    MethodSpec::request("provider.sessions.preview.get.request"),
    MethodSpec::request("agent.import.request"),
    MethodSpec::request("agent.refresh.request"),
    MethodSpec::request("agent.fork_context.request"),
    MethodSpec::request("agent.context.compact.request"),
    MethodSpec::request("agent.rewind.request"),
    MethodSpec::request("agent.commands.list.request"),
    MethodSpec::request("agent.mode.set.request"),
    MethodSpec::request("agent.feature.set.request"),
    MethodSpec::request("agent.permission.resolve.request"),
    MethodSpec::request("agent.provider_subagents.list.request"),
    MethodSpec::request("agent.subagents.list.request"),
    MethodSpec::request("agent.provider_subagents.timeline.get.request"),
    MethodSpec::request("provider.diagnostic.request"),
    MethodSpec::request("provider.presets.describe.request"),
    MethodSpec::request("provider.presets.list.request"),
    MethodSpec::request("provider.presets.save.request"),
    MethodSpec::request("provider.presets.delete.request"),
    MethodSpec::request("provider.usage.ledger.request"),
    MethodSpec::request("agent.side_question.ask.request"),
    MethodSpec::request("agent.side_question.cancel.request"),
    MethodSpec::request("agent.side_question.history.request"),
    MethodSpec::request("agent.side_question.transfer.request"),
];

use model::ErrorCode;
use model::methods::MethodSpec;
use serde_json::Value;
use tokio_util::task::task_tracker::TaskTrackerToken;

use crate::dispatch::State as Shared;
use crate::service::agent_execution::{AdmittedResponse, AgentExecution};
use model::Context;
use model::outbound::{Outbound, QueueError};

/// Admit ordered native work without holding the connection's capability worker.
/// Response ownership and task tracking remain bounded until delivery.
/// # Errors
/// Returns delivery failure; admission and execution errors use the original request ID.
pub(super) fn request(context: Context<'_>, state: &Shared) -> Result<(), QueueError> {
    respond_admitted(context, state, AgentExecution::admit_request)
}

/// Admit `context` through `admit` on the installed executor, then deliver its response from a
/// tracked task so the connection's capability worker is never held.
///
/// # Arguments
/// * `context` - Pending request whose method and parameters are admitted.
/// * `state` - Shared admission, cancellation, task tracking and the installed executor.
/// * `admit` - Executor lane that reserves a bounded response slot for the request.
///
/// # Errors
/// Returns delivery failure; admission and execution errors use the original request ID.
pub(super) fn respond_admitted(
    context: Context<'_>,
    state: &Shared,
    admit: fn(&AgentExecution, &str, Value) -> Result<AdmittedResponse, crate::rpc::ErrorCode>,
) -> Result<(), QueueError> {
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
    let mut response = match admit(execution, &context.request.method, context.request.params) {
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
    // outbound delivery. Dropping the socket cannot cancel accepted work, such as a catalog
    // cache or refresh update.
    state.tasks.spawn(async move {
        let result = response.receive().await.map_err(Into::into);
        let _ = outbound.respond(request_id, result);
        drop(response);
    });
    drop(admission);
    Ok(())
}

pub(crate) fn wait(
    request_id: String,
    params: Value,
    state: &Shared,
    outbound: &Outbound,
) -> Result<(), QueueError> {
    let admission = state
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let permit = state.execution_waits.clone().try_acquire_owned();
    let error = if state.cancellation.is_cancelled() {
        Some(ErrorCode::ServerDraining)
    } else if state.agent_execution.is_none() {
        Some(ErrorCode::UnsupportedCapability)
    } else if permit.is_err() {
        Some(ErrorCode::ResourceExhausted)
    } else {
        None
    };
    if let Some(code) = error {
        return outbound.respond(request_id, Err(code));
    }
    let execution = state
        .agent_execution
        .clone()
        .expect("installed execution checked above");
    let outbound = outbound.clone();
    let cancellation = state.cancellation.clone();
    state.tasks.spawn(async move {
        let _permit = permit;
        let failure = outbound.failure();
        let result = tokio::select! {
            () = cancellation.cancelled() => return,
            () = failure.cancelled() => return,
            result = execution.execute("agent.finish.wait.request", params) => result,
        };
        let _ = outbound.respond(request_id, result.map_err(Into::into));
    });
    drop(admission);
    Ok(())
}

/// Summarize chat history for `agent.context.compact.request` with the isolated generator.
/// # Errors
/// Returns `ServerDraining` after shutdown begins, `InvalidMessage` for a malformed or oversized
/// request, and `UnsupportedCapability` when no generator is installed or generation fails.
pub(crate) async fn compact(params: Value, state: &Shared) -> Result<Value, ErrorCode> {
    let tracking = track(state)?;
    let request: crate::protocol::native_sessions::ContextCompactRequest =
        serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
    if request.context.len() > 96 * 1024
        || request.cwd.trim().is_empty()
        || request.provider.trim().is_empty()
        || request
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
    {
        return Err(ErrorCode::InvalidMessage);
    }
    let source = state
        .summary_source
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let summary = source
        .generate(domain::summary::SummaryRequest {
            kind: domain::summary::SummaryKind::ChatHistory,
            cwd: request.cwd,
            context: request.context,
            selection: Some(domain::summary::SummarySelection {
                provider: request.provider,
                model: request.model,
                thinking_option_id: request.thinking_option_id,
            }),
        })
        .await
        .map_err(|_| ErrorCode::UnsupportedCapability)?;
    drop(tracking);
    Ok(serde_json::json!({"summary":summary["summary"]}))
}

/// Execute `method` on the installed native executor while tracking it for shutdown.
/// # Errors
/// Returns `UnsupportedCapability` without an executor, `ServerDraining` after shutdown
/// begins, or the executor's failure.
pub(crate) async fn dispatch(
    method: &str,
    params: Value,
    state: &Shared,
) -> Result<Value, ErrorCode> {
    let execution = state
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let tracking = track(state)?;
    let result = execution.execute(method, params).await.map_err(Into::into);
    drop(tracking);
    result
}

/// Track one foreground operation so shutdown drains it, unless the server is already draining.
/// # Errors
/// Returns `AgentIo` for a poisoned admission lock and `ServerDraining` after shutdown begins.
fn track(state: &Shared) -> Result<TaskTrackerToken, ErrorCode> {
    let _admission = state.admission.lock().map_err(|_| ErrorCode::AgentIo)?;
    if state.cancellation.is_cancelled() {
        return Err(ErrorCode::ServerDraining);
    }
    Ok(state.tasks.token())
}
