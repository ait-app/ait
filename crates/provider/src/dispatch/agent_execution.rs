/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[&str] = &[
    "agent.create.request",
    "agent.resume.request",
    "agent.message.send.request",
    "agent.cancel.request",
    "agent.finish.wait.request",
    "agent.model.set.request",
    "agent.thinking.set.request",
    "agent.config.apply.request",
    "provider.sessions.recent.list.request",
    "agent.import.request",
    "agent.refresh.request",
    "agent.fork_context.request",
    "agent.rewind.request",
    "agent.commands.list.request",
    "agent.mode.set.request",
    "agent.feature.set.request",
    "agent.permission.resolve.request",
    "agent.provider_subagents.list.request",
    "agent.provider_subagents.timeline.get.request",
    "provider.diagnostic.request",
    "provider.usage.list.request",
];

use model::ErrorCode;
use model::ServerMessage;
use serde_json::Value;

use crate::dispatch::State as Shared;
use model::outbound::{Outbound, QueueError};

pub fn wait(
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
        return send_error(outbound, request_id, code);
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
        let _ = match result {
            Ok(result) => outbound.send(&ServerMessage::Response { request_id, result }),
            Err(code) => send_error(&outbound, request_id, code.into()),
        };
    });
    drop(admission);
    Ok(())
}

fn send_error(outbound: &Outbound, request_id: String, code: ErrorCode) -> Result<(), QueueError> {
    outbound.send(&ServerMessage::Error {
        request_id: Some(request_id),
        code,
        message: code.message().to_owned(),
        retryable: code.retryable(),
    })
}

pub async fn dispatch(method: &str, params: Value, state: &Shared) -> Result<Value, ErrorCode> {
    let execution = state
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let tracking = {
        let _admission = state.admission.lock().map_err(|_| ErrorCode::AgentIo)?;
        if state.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        state.tasks.token()
    };
    let result = execution.execute(method, params).await.map_err(Into::into);
    drop(tracking);
    result
}
