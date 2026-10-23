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
    MethodSpec::request("agent.provider_subagents.timeline.get.request"),
    MethodSpec::request("provider.diagnostic.request"),
    MethodSpec::request("provider.usage.list.request"),
];

use model::ErrorCode;
use model::ServerMessage;
use model::methods::MethodSpec;
use serde_json::Value;

use crate::dispatch::State as Shared;
use model::outbound::{Outbound, QueueError};

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

pub(crate) async fn dispatch(
    method: &str,
    params: Value,
    state: &Shared,
) -> Result<Value, ErrorCode> {
    if method == "agent.context.compact.request" {
        let tracking = {
            let _admission = state.admission.lock().map_err(|_| ErrorCode::AgentIo)?;
            if state.cancellation.is_cancelled() {
                return Err(ErrorCode::ServerDraining);
            }
            state.tasks.token()
        };
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
        return Ok(serde_json::json!({"summary":summary["summary"]}));
    }
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
