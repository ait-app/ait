//! One hub session bridged to its own WebSocket on the fixed local daemon.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{Bytes, Message, Utf8Bytes};
use uuid::Uuid;

use super::wire::PayloadKind;
use crate::protocol::{self, ClientHello};
use crate::transport::{MAX_MESSAGE, send};
use crate::{Local, diagnostics};

/// The daemon must answer the business hello promptly.
const FIRST_REPLY: Duration = Duration::from_secs(10);
const WRITE: Duration = Duration::from_secs(5);

/// Session output for the link, in daemon order.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Output {
    /// The daemon answered the hello; carries its first text reply.
    Accept(Bytes),
    Message(PayloadKind, Bytes),
    /// The bridge ended; the code is relayed to the access endpoint.
    Closed(&'static str),
}

/// Bridge session `id` until either side closes, then report the closure.
///
/// Dropping the sender of `inbound` closes the local daemon connection.
pub(super) async fn run(
    local: Arc<Local>,
    id: Uuid,
    hello: Bytes,
    guest: Option<String>,
    inbound: mpsc::Receiver<Message>,
    output: mpsc::Sender<(Uuid, Output)>,
) {
    let code = bridge(&local, id, hello, guest.as_deref(), inbound, &output)
        .await
        .err()
        .unwrap_or("closed");
    let _ = output.send((id, Output::Closed(code))).await;
}

/// A guest session dials with the encoded visitor; the local API confines it to
/// one workspace and refuses it once the share is gone.
async fn bridge(
    local: &Local,
    id: Uuid,
    hello: Bytes,
    guest: Option<&str>,
    mut inbound: mpsc::Receiver<Message>,
    output: &mpsc::Sender<(Uuid, Output)>,
) -> Result<(), &'static str> {
    let hello = Utf8Bytes::try_from(hello).map_err(|_| "invalid_hello")?;
    let _: ClientHello = protocol::decode(&hello).map_err(|_| "invalid_hello")?;
    let mut daemon = local
        .dialer
        .connect_as(&local.url, local.token.expose_secret(), guest, MAX_MESSAGE)
        .await
        .map_err(|_| {
            if guest.is_some() {
                "share_revoked"
            } else {
                "unavailable"
            }
        })?;
    send(&mut daemon, Message::Text(hello))
        .await
        .map_err(|_| "unavailable")?;
    let (mut sink, mut stream) = daemon.split();
    let accept = timeout(FIRST_REPLY, async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => return Ok(Output::Accept(Bytes::from(text))),
                Some(Ok(Message::Ping(body))) => {
                    sink.send(Message::Pong(body))
                        .await
                        .map_err(|_| "unavailable")?;
                }
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err("unavailable"),
            }
        }
    })
    .await
    .map_err(|_| "timeout")??;
    if output.send((id, accept)).await.is_err() {
        return Ok(());
    }
    loop {
        tokio::select! {
            message = stream.next() => {
                let forwarded = match message {
                    Some(Ok(Message::Text(text))) => Output::Message(PayloadKind::Text, Bytes::from(text)),
                    Some(Ok(Message::Binary(body))) => Output::Message(PayloadKind::Binary, body),
                    Some(Ok(Message::Ping(body))) => {
                        write(&mut sink, Message::Pong(body)).await?;
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => continue,
                    Some(Err(error)) => {
                        diagnostics::socket_error("hub_local_read", &error);
                        return Err("unavailable");
                    }
                    Some(Ok(Message::Close(_) | Message::Frame(_))) | None => return Ok(()),
                };
                // Waiting here pauses local reads while the link applies backpressure.
                if output.send((id, forwarded)).await.is_err() {
                    return Ok(());
                }
            }
            message = inbound.recv() => {
                let Some(message) = message else {
                    let _ = timeout(Duration::from_secs(1), sink.close()).await;
                    return Ok(());
                };
                write(&mut sink, message).await?;
            }
        }
    }
}

async fn write(
    sink: &mut futures_util::stream::SplitSink<crate::transport::Socket, Message>,
    message: Message,
) -> Result<(), &'static str> {
    match timeout(WRITE, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            diagnostics::socket_error("hub_local_write", &error);
            Err("unavailable")
        }
        Err(_) => Err("unavailable"),
    }
}

/// Convert a hub payload into the business WebSocket message it carried.
///
/// # Errors
/// Returns `invalid_message` for text payloads that are not UTF-8.
pub(super) fn message(kind: PayloadKind, payload: Bytes) -> Result<Message, &'static str> {
    match kind {
        PayloadKind::Text => Utf8Bytes::try_from(payload)
            .map(Message::Text)
            .map_err(|_| "invalid_message"),
        PayloadKind::Binary => Ok(Message::Binary(payload)),
    }
}

#[cfg(test)]
mod tests;
