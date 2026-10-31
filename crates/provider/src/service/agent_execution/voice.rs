use super::operation::Operation;
use super::{AgentExecution, CancellationToken, ErrorCode, ExecutionState, Value, json, oneshot};

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

type Reply = oneshot::Sender<Result<Value, ErrorCode>>;

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
    let abandoned = || {
        cancel.as_ref().is_some_and(CancellationToken::is_cancelled)
            || cancel.is_some() && reply.is_closed()
    };
    let result = if abandoned() {
        Err(ErrorCode::AgentIo)
    } else if let Some(endpoint) = realtime {
        state.start_realtime(params, endpoint).await
    } else {
        state.execute(method, params).await
    };
    if abandoned() {
        cancel_accepted(state, &result).await;
    }
    let _ = state.manager.reconcile().await;
    let _ = state.publish();
    if let Err(result) = reply.send(result)
        && cancel.is_some()
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
