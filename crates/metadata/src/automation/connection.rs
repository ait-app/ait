//! Workspace-scoped setup transcripts with a paused, ordered bootstrap, and script run waits.

use std::sync::{Arc, Mutex};

use domain::session::SessionError;
use model::outbound::QueueError;
use model::runtime::Runtime;
use model::{Context, ErrorCode, ServerMessage};
use serde_json::{Value, json};

use crate::automation::protocol::{WorkspaceScriptWaitRequest, WorkspaceSetupRequest};
use crate::automation::rpc;
use crate::automation::service::WorkspaceAutomation;
use crate::connection::Connection;
use crate::dispatch::State;

/// Admit one script run wait and answer it from a tracked task once the run finishes or the
/// bound elapses, so the connection keeps serving other requests meanwhile.
///
/// # Errors
/// Returns a queue failure when an immediate refusal cannot be delivered.
pub(crate) fn wait(context: Context<'_>, state: &State) -> Result<(), QueueError> {
    let runtime = state.runtime.clone();
    let admission = runtime
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let permit = runtime.execution_waits.clone().try_acquire_owned();
    let admitted = (|| {
        if runtime.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        let automation = state
            .workspace_automation
            .clone()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let request: WorkspaceScriptWaitRequest =
            serde_json::from_value(context.request.params.clone())
                .map_err(|_| ErrorCode::InvalidMessage)?;
        let bound = rpc::wait_bound(&request).map_err(ErrorCode::from)?;
        Ok((automation, request, bound))
    })()
    .and_then(|admitted| {
        permit
            .map(|permit| (admitted, permit))
            .map_err(|_| ErrorCode::ResourceExhausted)
    });
    let ((automation, request, bound), permit) = match admitted {
        Ok(admitted) => admitted,
        Err(code) => {
            drop(admission);
            return context.respond(Err(code));
        }
    };
    let outbound = context.outbound.clone();
    let request_id = context.request.id;
    let task_runtime = runtime.clone();
    runtime.tasks.spawn(async move {
        let _permit = permit;
        let failure = outbound.failure();
        let result = tokio::select! {
            () = task_runtime.cancellation.cancelled() => return,
            () = failure.cancelled() => return,
            result = wait_for_run(&task_runtime, automation, request, bound) => result,
        };
        let _ = outbound.respond(request_id, result);
    });
    drop(admission);
    Ok(())
}

async fn wait_for_run(
    runtime: &Runtime,
    automation: Arc<Mutex<WorkspaceAutomation>>,
    request: WorkspaceScriptWaitRequest,
    bound: std::time::Duration,
) -> Result<Value, ErrorCode> {
    let observed = request.clone();
    let completion = runtime
        .run_queued(
            Some(automation.clone()),
            ErrorCode::RegistryIo,
            move |service| Ok(rpc::run_completion(service, &observed)),
        )
        .await?;
    if let Some(completion) = completion {
        // An elapsed bound is reported by the projected run, which is then still running.
        let _ = tokio::time::timeout(bound, completion.finished()).await;
    }
    runtime
        .run_queued(Some(automation), ErrorCode::RegistryIo, move |service| {
            rpc::wait_result(service, &request).map_err(Into::into)
        })
        .await
}

pub(crate) async fn subscribe(
    mut context: Context<'_>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    let prepared = (|| {
        if context.available_subscriptions == 0 {
            return Err(ErrorCode::ResourceExhausted);
        }
        if state.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        if state.workspace_automation.is_none() {
            return Err(ErrorCode::UnsupportedCapability);
        }
        let request: WorkspaceSetupRequest = serde_json::from_value(context.request.params.clone())
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let sink = context.outbound.clone();
        connection
            .session
            .as_ref()
            .ok_or(ErrorCode::InvalidMessage)?
            .subscribe_workspace_setup(
                request.workspace_id,
                Arc::new(move |kind, params| {
                    sink.send(&ServerMessage::Event {
                        method: kind.method().to_owned(),
                        params,
                    })
                    .map_err(|_| SessionError::Closed)
                }),
            )
            .map_err(|_| ErrorCode::InvalidMessage)
    })();
    let subscription = match prepared {
        Ok(subscription) => subscription,
        Err(error) => return context.respond(Err(error)),
    };
    let result = context
        .call(
            state.workspace_automation.clone(),
            ErrorCode::RegistryIo,
            |service, _, params| rpc::execute(service, "workspace.setup.status.request", params),
        )
        .await;
    let mut snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => return context.respond(Err(error)),
    };
    let id = subscription.id().to_owned();
    snapshot["subscriptionId"] = json!(id);
    let outbound = context.outbound;
    context.respond(Ok(snapshot))?;
    if subscription.activate().is_err() {
        outbound.failure().cancel();
        return Err(QueueError::Full);
    }
    connection.events.insert(id, subscription);
    Ok(())
}
