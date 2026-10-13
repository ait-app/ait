use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::diagnostics;
use crate::protocol::{ClientHello, Pairing};
use crate::transport::{MAX_MESSAGE, Socket, receive, send, text};
use crate::{Error, Local, protocol};

/// Pair `session` at `url` using `ticket`, then forward frames to the fixed local daemon.
///
/// # Errors
/// Returns protocol errors for invalid pairing or hello, and transport errors for setup failures.
pub(super) async fn data(
    url: String,
    ticket: SecretString,
    local: Arc<Local>,
    session: Uuid,
) -> Result<(), Error> {
    let mut remote = local
        .dialer
        .connect(&url, ticket.expose_secret(), MAX_MESSAGE)
        .await
        .inspect_err(|error| diagnostics::setup_error("remote_connect", error))?;
    let ready: Pairing = timeout(Duration::from_secs(30), receive(&mut remote))
        .await
        .map_err(|_| {
            tracing::warn!(
                stage = "pairing",
                reason = "timeout",
                timeout_ms = 30000,
                "Relay setup failed"
            );
            Error::Transport
        })
        .and_then(std::convert::identity)
        .inspect_err(|error| diagnostics::setup_error("pairing", error))?;
    ready
        .verify(session)
        .inspect_err(|error| diagnostics::setup_error("pairing_identity", error))?;
    // The local server's 10-second hello clock starts only after the remote
    // client has supplied its hello, never while waiting for Internet pairing.
    let hello = timeout(Duration::from_secs(10), text(&mut remote))
        .await
        .map_err(|_| {
            tracing::warn!(
                stage = "client_hello",
                reason = "timeout",
                timeout_ms = 10000,
                "Relay setup failed"
            );
            Error::Transport
        })
        .and_then(std::convert::identity)
        .inspect_err(|error| diagnostics::setup_error("client_hello", error))?;
    let _: ClientHello = protocol::decode(&hello)
        .inspect_err(|error| diagnostics::setup_error("client_hello_decode", error))?;
    let mut host = local
        .dialer
        .connect(&local.url, local.token.expose_secret(), MAX_MESSAGE)
        .await
        .inspect_err(|error| diagnostics::setup_error("local_connect", error))?;
    send(&mut host, Message::Text(hello))
        .await
        .inspect_err(|error| diagnostics::setup_error("local_hello_write", error))?;
    let cancel = CancellationToken::new();
    let (to_host, from_remote) = mpsc::channel(2);
    let (to_remote, from_host) = mpsc::channel(2);
    tokio::join!(
        endpoint(remote, to_host, from_host, cancel.clone())
            .instrument(tracing::info_span!("relay_endpoint", side = "remote_relay")),
        endpoint(host, to_remote, from_remote, cancel)
            .instrument(tracing::info_span!("relay_endpoint", side = "local_daemon"))
    );
    Ok(())
}

async fn endpoint(
    socket: Socket,
    peer: mpsc::Sender<Message>,
    incoming: mpsc::Receiver<Message>,
    cancel: CancellationToken,
) {
    let started = Instant::now();
    let (sink, mut stream) = socket.split();
    let (pong_tx, pong_rx) = mpsc::channel(1);
    let reader = async {
        loop {
            let message = tokio::select! {
                () = cancel.cancelled() => break,
                message = stream.next() => match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        diagnostics::socket_error("read", &error);
                        break;
                    },
                    None => {
                        tracing::info!(reason = "eof", "Relay endpoint stopped");
                        break;
                    },
                },
            };
            match message {
                Message::Text(_) | Message::Binary(_) => {
                    match timeout(Duration::from_secs(5), peer.send(message)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => {
                            tracing::info!(reason = "peer_queue_closed", "Relay endpoint stopped");
                            break;
                        }
                        Err(_) => {
                            tracing::warn!(
                                reason = "peer_queue_timeout",
                                timeout_ms = 5000,
                                "Relay endpoint stopped"
                            );
                            break;
                        }
                    }
                }
                Message::Ping(bytes) => {
                    if let Err(error) = pong_tx.try_send(Message::Pong(bytes)) {
                        let reason = match error {
                            mpsc::error::TrySendError::Full(_) => "pong_queue_full",
                            mpsc::error::TrySendError::Closed(_) => "pong_queue_closed",
                        };
                        tracing::warn!(reason, "Relay endpoint stopped");
                        break;
                    }
                }
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    tracing::info!(
                        reason = "close_frame",
                        close_code = frame.as_ref().map(|f| u16::from(f.code)),
                        "Relay endpoint stopped"
                    );
                    break;
                }
                Message::Frame(_) => {
                    tracing::warn!(reason = "unexpected_frame", "Relay endpoint stopped");
                    break;
                }
            }
        }
        cancel.cancel();
    };

    tokio::join!(reader, write(sink, incoming, pong_rx, &cancel));
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis(),
        "Relay endpoint finished"
    );
}

async fn write(
    mut sink: futures_util::stream::SplitSink<Socket, Message>,
    mut incoming: mpsc::Receiver<Message>,
    mut pong_rx: mpsc::Receiver<Message>,
    cancel: &CancellationToken,
) {
    loop {
        let message = tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            Some(pong) = pong_rx.recv() => pong,
            message = incoming.recv() => if let Some(message) = message { message } else {
                tracing::info!(reason = "incoming_queue_closed", "Relay endpoint stopped");
                break;
            },
        };
        match timeout(Duration::from_secs(5), sink.send(message)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                diagnostics::socket_error("write", &error);
                break;
            }
            Err(_) => {
                tracing::warn!(
                    reason = "write_timeout",
                    timeout_ms = 5000,
                    "Relay endpoint stopped"
                );
                break;
            }
        }
    }
    cancel.cancel();
    let _ = timeout(Duration::from_secs(1), sink.close()).await;
}

#[cfg(test)]
mod tests;
