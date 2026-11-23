use super::operation::Operation;
use super::{AgentExecution, CancellationToken, ErrorCode, ExecutionState, Value, json};
use crate::rpc::agent_execution::Reply;
use tokio::sync::oneshot;

impl AgentExecution {
    /// Open native realtime audio for an Agent when its provider declares support.
    /// `agent` selects the target and `cancel` bounds its lifetime. Audio is always PCM at
    /// this boundary; the provider owns upstream negotiation and media transport.
    /// Returns `None` only when the provider declares no native support.
    /// The returned handle cancels native audio when dropped, including abandoned startup.
    /// # Errors
    /// Returns safe admission, identity or native errors; supported providers never fall back.
    pub async fn start_realtime(
        &self,
        agent: &str,
        cancel: CancellationToken,
    ) -> Result<Option<model::realtime::Session>, ErrorCode> {
        let (session, endpoint) = model::realtime::channel(&cancel);
        let result = self
            .call_with_realtime(
                Operation::Realtime,
                json!({"agentId":agent}),
                Some(cancel),
                Some(endpoint),
            )
            .await?;
        Ok((result["supported"] == true).then_some(session))
    }

    /// Admit a voice-owned turn while preserving cancellation across the worker queue.
    ///
    /// `agent` identifies the target, `text` is its prompt, and `cancel` belongs to the voice
    /// operation. Returns its admission receipt including the native turn ID. Cancellation
    /// during admission interrupts only that accepted turn, even if the caller stops waiting.
    /// # Errors
    /// Returns validation, queue, provider or cancellation errors using safe shared codes.
    pub async fn send_voice(
        &self,
        agent: &str,
        text: &str,
        cancel: CancellationToken,
    ) -> Result<Value, ErrorCode> {
        self.call_cancellable(
            Operation::VoiceSend,
            json!({"agentId":agent,"text":text}),
            Some(cancel),
        )
        .await
    }
}

pub(crate) struct PendingPreparation {
    id: String,
    request: crate::protocol::agent_execution::CreateRequest,
    prompt: Option<provider_core::protocol::prompt::AgentPrompt>,
    admission: domain::creation::protocol::Snapshot,
    workspace: String,
    created_worktree: bool,
    result: oneshot::Receiver<
        Result<
            (
                domain::agent_runtime::StoredAgentConfig,
                domain::agent_runtime::ModelPresetState,
            ),
            model::ErrorCode,
        >,
    >,
    reply: Option<Reply>,
    cancel: Option<CancellationToken>,
}

impl PendingPreparation {
    pub(crate) fn new(
        id: String,
        request: crate::protocol::agent_execution::CreateRequest,
        prompt: Option<provider_core::protocol::prompt::AgentPrompt>,
        admission: domain::creation::protocol::Snapshot,
        workspace: String,
        created_worktree: bool,
        result: oneshot::Receiver<
            Result<
                (
                    domain::agent_runtime::StoredAgentConfig,
                    domain::agent_runtime::ModelPresetState,
                ),
                model::ErrorCode,
            >,
        >,
    ) -> Self {
        Self {
            id,
            request,
            prompt,
            admission,
            workspace,
            created_worktree,
            result,
            reply: None,
            cancel: None,
        }
    }
}

pub(super) async fn settle_preparations(state: &mut ExecutionState) {
    let pending = std::mem::take(&mut state.pending_preparations);
    for mut item in pending {
        let cancelled = item
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled);
        let result = if cancelled {
            Some(Err(ErrorCode::AgentIo))
        } else {
            match item.result.try_recv() {
                Ok(result) => Some(result.map_err(ErrorCode::from)),
                Err(oneshot::error::TryRecvError::Closed) => Some(Err(ErrorCode::AgentIo)),
                Err(oneshot::error::TryRecvError::Empty) => None,
            }
        };
        let Some(result) = result else {
            state.pending_preparations.push(item);
            continue;
        };
        let response = match result {
            Ok((config, mut preset)) => {
                item.request.config.stored = config;
                let placement = if item.workspace.is_empty() {
                    state.place_prepared_creation(&mut item.request).await
                } else {
                    Ok((item.workspace.clone(), item.created_worktree))
                };
                match placement {
                    Ok((workspace, created_worktree)) => {
                        if let Some(reference) = &mut preset.selected {
                            reference.agent_cwd.clone_from(&item.request.config.cwd);
                        }
                        state
                            .register_prepared_creation(
                                item.id.clone(),
                                item.request,
                                item.prompt,
                                &item.admission,
                                (workspace, created_worktree),
                                Some(preset),
                            )
                            .await
                    }
                    Err(error) => {
                        state
                            .creation_preparation_failed(
                                &item.id,
                                "",
                                false,
                                &item.admission,
                                error,
                            )
                            .await
                    }
                }
            }
            Err(error) => {
                state
                    .creation_preparation_failed(
                        &item.id,
                        &item.workspace,
                        item.created_worktree,
                        &item.admission,
                        error,
                    )
                    .await
            }
        };
        if response
            .as_ref()
            .is_ok_and(|value| value.get("_aitPendingCreation").is_some())
        {
            if let Some(creation) = state
                .pending_creations
                .iter_mut()
                .find(|creation| creation.id == item.id && creation.reply.is_none())
            {
                creation.reply = item.reply.take();
                creation.cancel = item.cancel;
            }
        } else if let Some(reply) = item.reply.take() {
            let _ = reply.send(response);
        }
    }
}

pub(crate) struct PendingCreation {
    id: String,
    message: String,
    progress: domain::creation::protocol::Snapshot,
    reply: Option<Reply>,
    cancel: Option<CancellationToken>,
    started: std::time::Instant,
}

impl PendingCreation {
    pub(crate) fn new(
        id: String,
        message: String,
        progress: domain::creation::protocol::Snapshot,
    ) -> Self {
        Self {
            id,
            message,
            progress,
            reply: None,
            cancel: None,
            started: std::time::Instant::now(),
        }
    }
}

pub(super) fn settle_creations(state: &mut ExecutionState) {
    use crate::service::agent_manager::AgentManagerError;
    use domain::creation::protocol::Phase;
    let pending = std::mem::take(&mut state.pending_creations);
    for mut item in pending {
        let cancelled = item
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled);
        let timed_out = item.started.elapsed() >= std::time::Duration::from_secs(60);
        let message = &item.message;
        let Some(timeline) = state.manager.timeline() else {
            if let Some(reply) = item.reply.take() {
                let _ = reply.send(Err(ErrorCode::AgentIo));
            }
            continue;
        };
        if cancelled || timed_out {
            state.manager.discard_preset_refresh(&item.id);
            let _ = timeline.cancel_queued_input(&item.id, message);
        }
        let delivery = timeline.input_delivery_state(&item.id, message);
        if matches!(
            delivery.as_ref().ok().and_then(Option::as_deref),
            Some("queued" | "claimed")
        ) && !cancelled
            && !timed_out
        {
            state.pending_creations.push(item);
            continue;
        }
        let result: Result<(), AgentManagerError> = if matches!(
            delivery.as_ref().ok().and_then(Option::as_deref),
            Some("accepted")
        ) {
            Ok(())
        } else {
            Err(AgentManagerError::Session)
        };
        let creations = state.manager.creations();
        let response = match result {
            Ok(()) => (|| {
                let progress =
                    creations.advance(&item.progress, Phase::PromptStarted, None, None)?;
                let snapshot = state.snapshot(&item.id)?;
                let progress = creations.advance(
                    &progress,
                    Phase::Completed,
                    Some(json!({"agent":snapshot})),
                    None,
                )?;
                Ok(
                    json!({"status":"agent_created","agentId":item.id,"agent":snapshot,"error":null,"creation":progress}),
                )
            })(),
            Err(error) => {
                let _ =
                    creations.advance(&item.progress, Phase::Failed, None, Some(error.to_string()));
                Err(ErrorCode::AgentIo)
            }
        };
        if let Some(reply) = item.reply.take() {
            let _ = reply.send(response);
        }
    }
}

pub(crate) struct Pending {
    params: Value,
    reply: Reply,
    cancel: Option<CancellationToken>,
    started: std::time::Instant,
}

pub(super) async fn settle(state: &mut ExecutionState) {
    let pending = std::mem::take(&mut state.pending_voices);
    for item in pending {
        if item.reply.is_closed()
            || item
                .cancel
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            || item.started.elapsed() >= std::time::Duration::from_secs(60)
        {
            if let Some(id) = item.params["agentId"].as_str() {
                state.manager.discard_preset_refresh(id);
            }
            let _ = item.reply.send(Err(ErrorCode::AgentIo));
            continue;
        }
        let result = state
            .execute(Operation::VoiceSend, item.params.clone())
            .await;
        if matches!(result, Err(ErrorCode::CatalogBusy)) {
            state.pending_voices.push(item);
            continue;
        }
        if item
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
            || item.reply.is_closed()
        {
            cancel_accepted(state, &result).await;
        }
        if let Err(result) = item.reply.send(result) {
            cancel_accepted(state, &result).await;
        }
    }
}

pub(super) async fn dispatch(
    state: &mut ExecutionState,
    method: Operation,
    params: Value,
    context: (
        Reply,
        Option<CancellationToken>,
        Option<model::realtime::Endpoint>,
    ),
) {
    let (reply, cancel, realtime) = context;
    if method == Operation::ModelPreset {
        state.begin_preset_selection(&params, reply);
        return;
    }
    let abandoned = || {
        cancel.as_ref().is_some_and(CancellationToken::is_cancelled)
            || cancel.is_some() && reply.is_closed()
    };
    let retry_params = (method == Operation::VoiceSend).then(|| params.clone());
    // A child control parks its reply under the adapter receipt and answers from a later drain,
    // so this lane is free to serve queued permissions and reads while the native runtime works.
    if realtime.is_none() && matches!(method, Operation::ChildGuide | Operation::ChildStop) {
        if abandoned() {
            let _ = reply.send(Err(ErrorCode::AgentIo));
        } else {
            state.defer_child_control(method, params, reply);
        }
        let _ = state.manager.reconcile().await;
        let _ = state.publish();
        return;
    }
    let result = if abandoned() {
        Err(ErrorCode::AgentIo)
    } else if let Some(endpoint) = realtime {
        state.start_realtime(params, endpoint).await
    } else {
        state.execute(method, params).await
    };
    if let Ok(value) = &result
        && let Some(id) = value["_aitPendingPreparation"].as_str()
        && let Some(pending) = state
            .pending_preparations
            .iter_mut()
            .find(|pending| pending.id == id && pending.reply.is_none())
    {
        pending.reply = Some(reply);
        pending.cancel = cancel;
        return;
    }
    if let Ok(value) = &result
        && let Some(id) = value["_aitPendingCreation"].as_str()
        && let Some(pending) = state
            .pending_creations
            .iter_mut()
            .find(|pending| pending.id == id && pending.reply.is_none())
    {
        pending.reply = Some(reply);
        pending.cancel = cancel;
        return;
    }
    if matches!(result, Err(ErrorCode::CatalogBusy))
        && let Some(params) = retry_params
        && !abandoned()
    {
        if state.pending_voices.len() >= 4 {
            let _ = reply.send(Err(ErrorCode::CatalogBusy));
        } else {
            state.pending_voices.push(Pending {
                params,
                reply,
                cancel,
                started: std::time::Instant::now(),
            });
        }
        return;
    }
    let cancelled = abandoned();
    if cancelled {
        cancel_accepted(state, &result).await;
    }
    let _ = state.manager.reconcile().await;
    let _ = state.publish();
    if let Err(result) = reply.send(result)
        && cancel.is_some()
        && !cancelled
    {
        cancel_accepted(state, &result).await;
    }
}

async fn cancel_accepted(state: &mut ExecutionState, result: &Result<Value, ErrorCode>) {
    if let Ok(receipt) = result
        && receipt["accepted"] == true
    {
        let _ = state
            .execute(
                Operation::VoiceCancel,
                json!({"agentId":receipt["agentId"],"turnId":receipt["turnId"]}),
            )
            .await;
    }
}
