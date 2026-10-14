//! One-shot transport adapter for the existing capability dispatch chain.

use model::outbound::{Frame, Outbound};
use model::{Context, ErrorCode, Request, ServerMessage};
use serde_json::Value;

use super::ConnectionSubscriptions;
use crate::Shared;

/// Dispatch `method` and `params` against `state`, returning its correlated business result.
/// Subscriptions are unavailable and the private output channel is cancelled on completion.
/// # Errors
/// Returns existing RPC errors, malformed replies or exhausted output admission.
pub(crate) async fn execute(
    state: &Shared,
    method: &str,
    params: Value,
) -> Result<Value, ErrorCode> {
    let (outbound, mut receiver) = Outbound::new();
    let _lifetime = OutputLifetime(outbound.clone());
    let id = uuid::Uuid::new_v4().to_string();
    let request = Request {
        id: id.clone(),
        method: method.to_owned(),
        params,
    };
    let mut subscriptions = ConnectionSubscriptions::default();
    let dispatch = super::dispatch::request(
        Context {
            request,
            runtime: state,
            outbound: &outbound,
            available_subscriptions: 0,
        },
        state,
        &mut subscriptions,
    );
    let receive = async {
        while let Some(queued) = receiver.recv().await {
            let Frame::Text(text) = &queued.message else {
                continue;
            };
            let message: ServerMessage =
                serde_json::from_str(text).map_err(|_| ErrorCode::InvalidMessage)?;
            match message {
                ServerMessage::Response { request_id, result } if request_id == id => {
                    return Ok(result);
                }
                ServerMessage::Error {
                    request_id, code, ..
                } if request_id.as_deref() == Some(&id) => return Err(code),
                _ => {}
            }
        }
        Err(ErrorCode::ResourceExhausted)
    };
    tokio::pin!(dispatch, receive);
    tokio::select! {
        result = &mut receive => result,
        result = &mut dispatch => {
            result.map_err(|_| ErrorCode::ResourceExhausted)?;
            receive.await
        }
    }
}

struct OutputLifetime(Outbound);

impl Drop for OutputLifetime {
    fn drop(&mut self) {
        self.0.failure().cancel();
    }
}
