use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use crate::protocol::{DownloadAck, DownloadMessage, Pairing};
use crate::transport::{MAX_MESSAGE, Socket, connect, receive};
use crate::{Error, Local, protocol};

/// Pair `session` using its relay ticket and stream the local file authorized by `token`.
///
/// # Errors
/// Rejects invalid grants or HTTP responses, failed I/O, and a missing destination acknowledgement.
pub(super) async fn run(
    url: String,
    ticket: SecretString,
    token: SecretString,
    local: Arc<Local>,
    session: Uuid,
) -> Result<(), Error> {
    protocol::validate_download_token(token.expose_secret())?;
    let mut socket = connect(&url, ticket.expose_secret(), MAX_MESSAGE).await?;
    let ready: Pairing = timeout(Duration::from_secs(30), receive(&mut socket))
        .await
        .map_err(|_| Error::Transport)??;
    ready.verify(session)?;
    let response = local_response(&local, token.expose_secret()).await?;
    let headers = protocol::encode(&DownloadMessage::Headers {
        status: response.status().as_u16(),
        content_length: response.content_length(),
        content_type: response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok()),
    })?;
    let (mut sink, incoming) = socket.split();
    let (pong_tx, mut pong_rx) = mpsc::channel(1);
    let (complete_tx, complete_rx) = oneshot::channel();
    let cancel = CancellationToken::new();
    let reader = read_ack(incoming, pong_tx, complete_tx, &cancel);
    let writer = async {
        timeout(
            Duration::from_secs(5),
            sink.send(Message::Text(headers.into())),
        )
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
        let mut body = response.bytes_stream();
        let mut written = 0_u64;
        loop {
            let chunk = tokio::select! {
                () = cancel.cancelled() => return Err(Error::Transport),
                Some(pong) = pong_rx.recv() => {
                    timeout(Duration::from_secs(5), sink.send(pong)).await.map_err(|_| Error::Transport)?.map_err(|_| Error::Transport)?;
                    continue;
                },
                next = timeout(Duration::from_secs(30), body.next()) => next.map_err(|_| Error::Transport)?,
            };
            let Some(chunk) = chunk else { break };
            let bytes = chunk.map_err(|_| Error::Transport)?;
            for chunk in bytes.chunks(64 * 1024) {
                timeout(
                    Duration::from_secs(5),
                    sink.send(Message::Binary(chunk.to_vec().into())),
                )
                .await
                .map_err(|_| Error::Transport)?
                .map_err(|_| Error::Transport)?;
                written += chunk.len() as u64;
            }
        }
        timeout(
            Duration::from_secs(5),
            sink.send(Message::Text(
                protocol::encode(&DownloadMessage::End { bytes: written })?.into(),
            )),
        )
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
        // Do not close ahead of queued frames at the center. The destination
        // acknowledges only after its final file write has completed.
        timeout(Duration::from_secs(10), complete_rx)
            .await
            .map_err(|_| Error::Transport)?
            .map_err(|_| Error::Transport)?;
        Ok::<_, Error>(())
    };
    let completing = async {
        let result = writer.await;
        cancel.cancel();
        result
    };
    let ((), result) = tokio::join!(reader, completing);
    result
}

async fn local_response(local: &Local, token: &str) -> Result<reqwest::Response, Error> {
    let mut download = Url::parse(&local.url).map_err(|_| Error::Protocol)?;
    download.set_scheme("http").map_err(|()| Error::Protocol)?;
    download.set_path("/api/files/download");
    download.query_pairs_mut().append_pair("token", token);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| Error::Transport)?;
    let response = timeout(Duration::from_secs(10), client.get(download).send())
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
    if !response.status().is_success() {
        return Err(Error::Protocol);
    }
    Ok(response)
}

async fn read_ack(
    mut incoming: futures_util::stream::SplitStream<Socket>,
    pong_tx: mpsc::Sender<Message>,
    complete_tx: oneshot::Sender<()>,
    cancel: &CancellationToken,
) {
    loop {
        let frame = tokio::select! {
            () = cancel.cancelled() => return,
            frame = incoming.next() => frame,
        };
        match frame {
            Some(Ok(Message::Ping(bytes))) => {
                if pong_tx.try_send(Message::Pong(bytes)).is_err() {
                    break;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Text(value))) => {
                if protocol::decode::<DownloadAck>(&value).is_ok() {
                    let _ = complete_tx.send(());
                    return;
                }
                break;
            }
            _ => break,
        }
    }
    cancel.cancel();
}

#[cfg(test)]
mod tests;
