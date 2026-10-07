//! Connection-owned, acknowledged assembly of bounded client JSON messages.
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::ws::WebSocket;
use futures_util::stream::SplitStream;
use model::outbound::Outbound;
use protocol::ErrorCode;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, timeout_at};

use super::{Incoming, receive};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const CHUNK_BYTES: usize = 256 * 1024;
const HEADER: usize = 13;
const DEADLINE: Duration = Duration::from_secs(60);
static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Default)]
pub(super) struct Assembly {
    pending: Option<Pending>,
    parsing: Option<tokio::task::JoinHandle<Result<Incoming, ErrorCode>>>,
}

struct Pending {
    id: u32,
    size: usize,
    bytes: Vec<u8>,
    permit: OwnedSemaphorePermit,
    deadline: Instant,
}

impl Assembly {
    fn push(&mut self, frame: &[u8]) -> Result<(serde_json::Value, Option<Pending>), ErrorCode> {
        if frame.len() <= HEADER || frame.len() > HEADER + CHUNK_BYTES {
            return Err(ErrorCode::InvalidMessage);
        }
        let word = |offset| {
            u32::from_be_bytes(
                frame[offset..offset + 4]
                    .try_into()
                    .expect("validated chunk header"),
            )
        };
        let id = word(1);
        let size = word(5) as usize;
        let offset = word(9) as usize;
        if size == 0 || size > MAX_BYTES || offset >= size {
            return Err(ErrorCode::InvalidMessage);
        }
        if self.pending.is_none() {
            if offset != 0 {
                return Err(ErrorCode::InvalidMessage);
            }
            let budget = BUDGET.get_or_init(|| Arc::new(Semaphore::new(256 * 1024 * 1024)));
            let permit = budget
                .clone()
                .try_acquire_many_owned(u32::try_from(size).expect("validated message size"))
                .map_err(|_| ErrorCode::ResourceExhausted)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size)
                .map_err(|_| ErrorCode::ResourceExhausted)?;
            self.pending = Some(Pending {
                id,
                size,
                bytes,
                permit,
                deadline: Instant::now() + DEADLINE,
            });
        }
        let pending = self.pending.as_mut().expect("chunk assembly exists");
        if pending.id != id
            || pending.size != size
            || pending.bytes.len() != offset
            || Instant::now() >= pending.deadline
            || frame.len() - HEADER > size - offset
        {
            return Err(ErrorCode::InvalidMessage);
        }
        pending.bytes.extend_from_slice(&frame[HEADER..]);
        let ack = serde_json::json!({"id": id, "offset": pending.bytes.len()});
        let complete = (pending.bytes.len() == size)
            .then(|| self.pending.take())
            .flatten();
        Ok((ack, complete))
    }
}

pub(super) async fn next(
    stream: &mut SplitStream<WebSocket>,
    assembly: &mut Assembly,
    outbound: &Outbound,
) -> Option<Result<Incoming, ErrorCode>> {
    loop {
        if let Some(parsing) = &mut assembly.parsing {
            let result = parsing.await.unwrap_or(Err(ErrorCode::InvalidMessage));
            assembly.parsing = None;
            return Some(result);
        }
        if assembly
            .pending
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.deadline)
        {
            assembly.pending = None;
            return Some(Err(ErrorCode::InvalidMessage));
        }
        let incoming = if let Some(pending) = &assembly.pending {
            if let Ok(message) = timeout_at(pending.deadline, receive(stream)).await {
                message?
            } else {
                assembly.pending = None;
                return Some(Err(ErrorCode::InvalidMessage));
            }
        } else {
            receive(stream).await?
        };
        let frame = match incoming {
            Ok(Incoming::Binary(frame)) if frame.first() == Some(&0x30) => frame,
            other => return Some(other),
        };
        let (ack, complete) = match assembly.push(&frame) {
            Ok(result) => result,
            Err(code) => {
                assembly.pending = None;
                return Some(Err(code));
            }
        };
        if outbound
            .send(&protocol::ServerMessage::Event {
                method: "connection.chunk.ack".to_owned(),
                params: ack,
            })
            .is_err()
        {
            return None;
        }
        if let Some(pending) = complete {
            // Retain the task in connection state so select cancellation cannot lose
            // a completed message. Keep its reservation through parsing and admission.
            assembly.parsing = Some(tokio::task::spawn_blocking(move || {
                serde_json::from_slice(&pending.bytes)
                    .map(|message| Incoming::Text(message, Some(pending.permit)))
                    .map_err(|_| ErrorCode::InvalidMessage)
            }));
        }
    }
}

#[cfg(test)]
mod tests;
