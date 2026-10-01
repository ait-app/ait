//! Bounded transport-neutral output queue.

use std::sync::Arc;

use crate::ServerMessage;
use crate::server::{MAX_QUEUE_BYTES, MAX_QUEUE_MESSAGES};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
/// Failure to encode or enqueue outgoing connection data.
pub enum QueueError {
    /// The JSON payload could not be serialized.
    #[error("outgoing message cannot be encoded")]
    Encode(#[from] serde_json::Error),
    /// The queue is full or the connection has closed.
    #[error("outgoing connection budget exhausted or connection closed")]
    Full,
}

#[derive(Debug)]
/// One queued frame that retains its byte budget through the active socket write.
pub struct Queued {
    /// Encoded frame to write.
    pub message: Frame,
    // Budget remains reserved until the active socket write completes or is dropped.
    _bytes: OwnedSemaphorePermit,
    _messages: Option<OwnedSemaphorePermit>,
}

#[derive(Clone, Debug)]
/// Cloneable bounded connection output.
pub struct Outbound {
    sender: mpsc::Sender<Queued>,
    bytes: Arc<Semaphore>,
    failed: CancellationToken,
    messages: Option<Arc<Semaphore>>,
}

impl Outbound {
    /// Create an output queue and its single transport receiver.
    #[must_use]
    pub fn new() -> (Self, mpsc::Receiver<Queued>) {
        let (sender, receiver) = mpsc::channel(MAX_QUEUE_MESSAGES);
        (
            Self {
                sender,
                bytes: Arc::new(Semaphore::new(MAX_QUEUE_BYTES)),
                failed: CancellationToken::new(),
                messages: None,
            },
            receiver,
        )
    }

    /// Create four fairly scheduled lanes sharing one physical byte/message budget.
    ///
    /// Frames retain admission through their active write; a failure cancels all lanes.
    #[must_use]
    pub fn fair() -> ([Self; 4], FairReceiver) {
        let bytes = Arc::new(Semaphore::new(MAX_QUEUE_BYTES));
        let messages = Arc::new(Semaphore::new(MAX_QUEUE_MESSAGES));
        let failed = CancellationToken::new();
        let mut receivers = Vec::with_capacity(4);
        let lanes = std::array::from_fn(|_| {
            let (sender, receiver) = mpsc::channel(MAX_QUEUE_MESSAGES);
            receivers.push(receiver);
            Self {
                sender,
                bytes: bytes.clone(),
                messages: Some(messages.clone()),
                failed: failed.clone(),
            }
        });
        (
            lanes,
            FairReceiver {
                receivers,
                cursor: 0,
            },
        )
    }

    /// Enqueue an encoded server message without waiting.
    /// # Errors
    /// Cancels the failure token if encoding or a queue budget fails.
    pub fn send(&self, message: &ServerMessage) -> Result<(), QueueError> {
        let result = self.try_send(message);
        if result.is_err() {
            self.failed.cancel();
        }
        result
    }

    /// Return the token cancelled when delivery fails or the connection ends.
    #[must_use]
    pub fn failure(&self) -> CancellationToken {
        self.failed.clone()
    }

    /// Send one correlated response or stable error envelope.
    /// # Errors
    /// Returns an encoding or queue failure.
    pub fn respond(
        &self,
        request_id: String,
        result: Result<serde_json::Value, crate::ErrorCode>,
    ) -> Result<(), QueueError> {
        match result {
            Ok(result) => self.send(&ServerMessage::Response { request_id, result }),
            Err(code) => self.send(&ServerMessage::Error {
                request_id: Some(request_id),
                code,
                message: code.message().to_owned(),
                retryable: code.retryable(),
            }),
        }
    }

    fn try_send(&self, message: &ServerMessage) -> Result<(), QueueError> {
        let text = serde_json::to_string(message)?;
        let count = u32::try_from(text.len()).map_err(|_| QueueError::Full)?;
        let messages = self
            .messages
            .as_ref()
            .map(|budget| budget.clone().try_acquire_owned())
            .transpose()
            .map_err(|_| QueueError::Full)?;
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(count)
            .map_err(|_| QueueError::Full)?;
        self.sender
            .try_send(Queued {
                message: Frame::Text(text),
                _bytes: bytes,
                _messages: messages,
            })
            .map_err(|_| QueueError::Full)
    }

    /// Queue binary data, waiting within the connection byte budget.
    /// # Errors
    /// Rejects oversized data and interrupted or closed delivery.
    pub async fn binary(&self, data: Vec<u8>) -> Result<(), QueueError> {
        let count = u32::try_from(data.len()).map_err(|_| QueueError::Full)?;
        if data.len() > MAX_QUEUE_BYTES {
            return Err(QueueError::Full);
        }
        let messages = if let Some(budget) = &self.messages {
            Some(tokio::select! {
                () = self.failed.cancelled() => return Err(QueueError::Full),
                permit = budget.clone().acquire_owned() => permit.map_err(|_| QueueError::Full)?,
            })
        } else {
            None
        };
        let bytes = tokio::select! {
            () = self.failed.cancelled() => return Err(QueueError::Full),
            permit = self.bytes.clone().acquire_many_owned(count) => permit.map_err(|_| QueueError::Full)?,
        };
        tokio::select! {
            () = self.failed.cancelled() => Err(QueueError::Full),
            result = self.sender.send(Queued { message: Frame::Binary(data), _bytes: bytes, _messages:messages }) => result.map_err(|_| QueueError::Full),
        }
    }
}

/// Round-robin receiver for independent capability lanes of a single socket.
#[derive(Debug)]
pub struct FairReceiver {
    receivers: Vec<mpsc::Receiver<Queued>>,
    cursor: usize,
}

impl FairReceiver {
    /// Receive the next lane's frame, preserving order within each lane.
    pub async fn recv(&mut self) -> Option<Queued> {
        match self.try_recv() {
            Ok(message) => return Some(message),
            Err(mpsc::error::TryRecvError::Disconnected) => return None,
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
        // poll_fn registers each channel waker without spawning forwarding tasks
        // or moving frames outside their existing shared admission budget.
        std::future::poll_fn(|context| {
            let mut closed = 0;
            for offset in 0..self.receivers.len() {
                let index = (self.cursor + offset) % self.receivers.len();
                match self.receivers[index].poll_recv(context) {
                    std::task::Poll::Ready(Some(message)) => {
                        self.cursor = (index + 1) % self.receivers.len();
                        return std::task::Poll::Ready(Some(message));
                    }
                    std::task::Poll::Ready(None) => closed += 1,
                    std::task::Poll::Pending => {}
                }
            }
            if closed == self.receivers.len() {
                std::task::Poll::Ready(None)
            } else {
                std::task::Poll::Pending
            }
        })
        .await
    }

    /// Receive an immediately available frame without waiting.
    ///
    /// # Errors
    /// Returns Empty when all lanes are empty, or Disconnected when all are closed.
    pub fn try_recv(&mut self) -> Result<Queued, mpsc::error::TryRecvError> {
        let mut closed = 0;
        for offset in 0..self.receivers.len() {
            let index = (self.cursor + offset) % self.receivers.len();
            match self.receivers[index].try_recv() {
                Ok(message) => {
                    self.cursor = (index + 1) % self.receivers.len();
                    return Ok(message);
                }
                Err(mpsc::error::TryRecvError::Disconnected) => closed += 1,
                Err(mpsc::error::TryRecvError::Empty) => {}
            }
        }
        Err(if closed == self.receivers.len() {
            mpsc::error::TryRecvError::Disconnected
        } else {
            mpsc::error::TryRecvError::Empty
        })
    }
}

#[cfg(test)]
mod tests;

/// Encoded connection data; the API converts this to a WebSocket frame.
#[derive(Debug, Clone)]
pub enum Frame {
    /// Serialized JSON response or event.
    Text(String),
    /// Encoded binary protocol frame.
    Binary(Vec<u8>),
}
