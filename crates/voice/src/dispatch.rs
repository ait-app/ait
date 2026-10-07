//! Speech request entry point; the API owns only physical transport and negotiation.

use std::sync::Arc;

use model::{Context, DispatchError, ErrorCode, Runtime};

use crate::{connection::Connection, service::Speech};

/// Concrete services and shared runtime used by speech request and event handlers.
#[derive(Debug)]
pub struct State {
    /// Common server task/admission resources.
    pub runtime: Arc<Runtime>,
    /// Optional installed speech service; its backend configuration may independently be disabled.
    pub speech: Option<Speech>,
}

/// Dispatch an admitted request to the connection's speech state.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business errors are delivered in the correlated response.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    let Some(mut context) = context.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "voice.mode.set.request"
                | "voice.abort.request"
                | "dictation.stream.start"
                | "dictation.stream.finish"
                | "dictation.stream.cancel"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

    let stopped = context.request.method == "voice.abort.request"
        || context.request.method == "voice.mode.set.request"
            && context.request.params["enabled"] == false;
    let result = match &state.speech {
        Some(speech) if !state.runtime.cancellation.is_cancelled() => connection
            .request(
                &context.request.method,
                std::mem::take(&mut context.request.params),
                speech,
            )
            .await
            .map_err(Into::into),
        Some(_) => Err(ErrorCode::ServerDraining),
        None => Err(ErrorCode::UnsupportedCapability),
    };
    let notify = stopped && result.is_ok();
    let outbound = context.outbound;
    context.respond(result)?;
    if notify {
        outbound.send(&model::ServerMessage::Event {
            method: "voice.input.state".to_owned(),
            params: serde_json::json!({"isSpeaking":false}),
        })?;
    }
    Ok(())
}
