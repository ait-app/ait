//! Per-visit framing and local daemon bridge, independent of other Yamux streams.

use std::{io, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt, io::AsyncReadExt};
use secrecy::ExposeSecret;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::{mux::bounded, wire};
use crate::{Local, protocol::ClientHello, transport};

/// Validate OPEN/START and the original hello before opening the fixed local daemon socket.
/// Returns on either endpoint closing, invalid records or bounded write/setup failures.
pub(super) async fn visit(
    mut stream: yamux::Stream,
    local: Arc<Local>,
    epoch: Uuid,
    tunnel_id: Uuid,
) -> io::Result<()> {
    let (kind, body) = bounded(wire::read(&mut stream)).await?;
    let open: wire::Open = serde_json::from_slice(&body).map_err(|_| wire::invalid())?;
    if kind != wire::OPEN
        || open.epoch != epoch
        || open.tunnel_id != tunnel_id
        || open.mode != "ait-rust-single-v1"
    {
        return Err(wire::invalid());
    }
    bounded(wire::write(&mut stream, wire::ACCEPT, &[])).await?;
    timeout(
        Duration::from_secs(30),
        wire::expect(&mut stream, wire::START),
    )
    .await
    .map_err(|_| wire::invalid())??;
    let (kind, hello) = timeout(Duration::from_secs(10), wire::read(&mut stream))
        .await
        .map_err(|_| wire::invalid())??;
    if kind != wire::TEXT {
        return Err(wire::invalid());
    }
    let _: ClientHello = serde_json::from_slice(&hello).map_err(|_| wire::invalid())?;
    // Do not start the local daemon's hello clock until the remote hello exists.
    let mut local_socket = local
        .dialer
        .connect(&local.url, local.token.expose_secret(), wire::MAX_BODY)
        .await
        .map_err(|_| wire::invalid())?;
    let hello = String::from_utf8(hello).map_err(|_| wire::invalid())?;
    transport::send(&mut local_socket, Message::Text(hello.into()))
        .await
        .map_err(|_| wire::invalid())?;
    let (mut local_sink, mut local_stream) = local_socket.split();
    let (mut reader, mut writer) = AsyncReadExt::split(stream);
    let (pong_tx, mut pong_rx) = mpsc::channel(1);
    let from_local = async {
        while let Some(message) = local_stream.next().await {
            match message.map_err(|_| wire::invalid())? {
                Message::Text(body) => {
                    bounded(wire::write(&mut writer, wire::TEXT, body.as_bytes())).await?;
                }
                Message::Binary(body) => {
                    bounded(wire::write(&mut writer, wire::BINARY, &body)).await?;
                }
                Message::Ping(body) => pong_tx.try_send(body).map_err(|_| wire::invalid())?,
                Message::Pong(_) => {}
                Message::Close(_) => {
                    bounded(wire::write(&mut writer, wire::CLOSE, &[])).await?;
                    return Ok(());
                }
                Message::Frame(_) => return Err(wire::invalid()),
            }
        }
        Ok(())
    };
    let to_local = async {
        // Keep the record read alive across hop-local Ping frames; read_exact is not cancel safe.
        loop {
            let read = wire::read(&mut reader);
            tokio::pin!(read);
            let (kind, body) = loop {
                tokio::select! {
                    result = &mut read => break result?,
                    Some(pong) = pong_rx.recv() => {
                        bounded(async { local_sink.send(Message::Pong(pong)).await.map_err(|_| wire::invalid()) }).await?;
                    },
                }
            };
            let message = match kind {
                wire::TEXT => {
                    Message::Text(String::from_utf8(body).map_err(|_| wire::invalid())?.into())
                }
                wire::BINARY => Message::Binary(body.into()),
                wire::CLOSE => return Ok(()),
                _ => return Err(wire::invalid()),
            };
            bounded(async { local_sink.send(message).await.map_err(|_| wire::invalid()) }).await?;
        }
    };
    tokio::select! { result = from_local => result, result = to_local => result }
}
