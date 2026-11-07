//! In-process Hub sessions: the relay's [`LocalApi`] port served by this API.
//!
//! Each session is one business connection with the admission, protocol and
//! guest confinement of an authenticated local WebSocket upgrade, carried over
//! channels instead of a loopback socket.

use std::sync::{Arc, Weak};

use axum::extract::ws::{Message, Utf8Bytes};
use futures_util::{SinkExt, StreamExt, stream};
use relay::hub::bridge::{Admission, Connection, Frame, LocalApi, Refused};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

use super::{Admitted, serve_frames};
use crate::Shared;

/// Messages buffered in each direction of one in-process connection.
const QUEUE: usize = 64;

/// The API as seen by the Hub serve link. It holds the API weakly: the API owns
/// the link, and a session outliving the API is refused.
#[derive(Debug)]
pub(crate) struct InProcess {
    shared: Weak<Shared>,
}

impl InProcess {
    pub(crate) fn new(shared: &Arc<Shared>) -> Self {
        Self {
            shared: Arc::downgrade(shared),
        }
    }
}

#[async_trait::async_trait]
impl LocalApi for InProcess {
    async fn connect(&self, admission: Admission<'_>) -> Result<Connection, Refused> {
        let state = self.shared.upgrade().ok_or(Refused)?;
        let role = match admission {
            Admission::Owner => Admitted::Owner,
            Admission::Guest(visitor) => Admitted::Guest(Box::new(
                crate::guest::admit_visitor(&state, visitor)
                    .await
                    .map_err(|_| Refused)?,
            )),
            Admission::Automation => {
                Admitted::Automation(state.automation.admit().map_err(|_| Refused)?)
            }
        };
        let admitted = state.admit_connection().map_err(|_| Refused)?;
        let (sender, inbound) = mpsc::channel(QUEUE);
        let (outbound, receiver) = mpsc::channel(QUEUE);
        let frames = stream::unfold(inbound, |mut inbound| async move {
            let frame = inbound.recv().await?;
            Some((message(frame), inbound))
        })
        .boxed();
        // Control frames have no in-process equivalent; closing drops the channel.
        let sink =
            PollSender::new(outbound).with_flat_map(|message| stream::iter(frame(message).map(Ok)));
        tokio::spawn(async move {
            let _admitted = admitted;
            Box::pin(serve_frames(sink, frames, state, role, None)).await;
        });
        Ok(Connection { sender, receiver })
    }
}

/// The business message a Hub frame carries; the link already validated text.
fn message(frame: Frame) -> Result<Message, axum::Error> {
    match frame {
        Frame::Text(text) => Utf8Bytes::try_from(text)
            .map(Message::Text)
            .map_err(axum::Error::new),
        Frame::Binary(bytes) => Ok(Message::Binary(bytes)),
    }
}

/// The Hub frame for an outbound business message, without control frames.
fn frame(message: Message) -> Option<Frame> {
    match message {
        Message::Text(text) => Some(Frame::Text(text.into())),
        Message::Binary(bytes) => Some(Frame::Binary(bytes)),
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) => None,
    }
}

#[cfg(test)]
mod tests;
