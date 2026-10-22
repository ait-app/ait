//! Bounded center calls, correlated independently of session forwarding and transport ACKs.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use model::hub::{CallError, Scope};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use super::{Connector, Link, Outbound, wire};
use crate::Error;

const MAX_CALLS: usize = 16;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub(super) struct Sender {
    queue: mpsc::Sender<Request>,
    slots: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

#[derive(Debug)]
pub(super) struct Request {
    id: Uuid,
    method: &'static str,
    params: Bytes,
    reply: oneshot::Sender<Result<Value, CallError>>,
    _slot: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

pub(super) struct Calls {
    pub(super) queue: mpsc::Receiver<Request>,
    pending: HashMap<Uuid, Request>,
}

impl Calls {
    pub(super) fn channel() -> (Sender, Self) {
        let (queue, receiver) = mpsc::channel(MAX_CALLS);
        (
            Sender {
                queue,
                slots: Arc::new(Semaphore::new(MAX_CALLS)),
                bytes: Arc::new(Semaphore::new(MAX_BYTES)),
            },
            Self {
                queue: receiver,
                pending: HashMap::new(),
            },
        )
    }

    pub(super) fn fresh(&mut self) {
        self.pending.clear(); // Dropping senders fails calls that belonged to the old logical link.
    }

    pub(super) fn prune(&mut self) {
        self.pending.retain(|_, request| !request.reply.is_closed());
    }

    pub(super) fn output(
        &mut self,
        request: Request,
        link: &mut Link,
    ) -> Result<Option<Bytes>, Error> {
        if request.reply.is_closed() {
            return Ok(None);
        }
        let frame = link.push(
            |seq| Outbound::Call {
                seq,
                id: request.id,
                method: request.method,
            },
            &request.params,
        )?;
        self.pending.insert(request.id, request);
        Ok(Some(frame))
    }

    pub(super) fn reply(&mut self, id: Uuid, payload: &[u8]) -> Result<(), Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum Reply {
            Result(Value),
            Error { code: String },
        }
        let reply = serde_json::from_slice::<Reply>(payload).map_err(|_| Error::Protocol)?;
        if let Some(request) = self.pending.remove(&id) {
            let result = match reply {
                Reply::Result(value) => Ok(value),
                Reply::Error { code } => Err(CallError::Rejected(code)),
            };
            let _ = request.reply.send(result);
        }
        Ok(())
    }
}

impl Connector {
    pub(super) async fn call_center(
        &self,
        scope: Scope,
        method: &'static str,
        params: Value,
    ) -> Result<Value, CallError> {
        let sender = {
            let state = self.state.lock().await;
            let publication = self.publication.borrow();
            if publication.scope.as_ref() != Some(&scope) || !publication.online {
                return Err(CallError::Unavailable);
            }
            if !publication.supports(method) {
                return Err(CallError::Unsupported);
            }
            state.calls.clone().ok_or(CallError::Unavailable)?
        };
        let slot = sender
            .slots
            .try_acquire_owned()
            .map_err(|_| CallError::Capacity)?;
        let params = serde_json::to_vec(&params).map_err(|_| CallError::Protocol)?;
        if params.len() > wire::MAX_PAYLOAD {
            return Err(CallError::Protocol);
        }
        let length = u32::try_from(params.len()).map_err(|_| CallError::Protocol)?;
        let bytes = sender
            .bytes
            .try_acquire_many_owned(length)
            .map_err(|_| CallError::Capacity)?;
        let (reply, receiver) = oneshot::channel();
        sender
            .queue
            .try_send(Request {
                id: Uuid::new_v4(),
                method,
                params: params.into(),
                reply,
                _slot: slot,
                _bytes: bytes,
            })
            .map_err(|_| CallError::Capacity)?;
        tokio::time::timeout(CALL_TIMEOUT, receiver)
            .await
            .map_err(|_| CallError::Unavailable)?
            .map_err(|_| CallError::Unavailable)?
    }
}

#[cfg(test)]
mod tests;
