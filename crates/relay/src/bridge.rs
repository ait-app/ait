use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::{Error, Local, MAX_MESSAGE, Socket, connect, send, text};

pub(super) async fn data(url: String, ticket: String, local: Arc<Local>) -> Result<(), Error> {
    let mut remote = connect(&url, &ticket, MAX_MESSAGE).await?;
    let ready = timeout(Duration::from_secs(30), text(&mut remote))
        .await
        .map_err(|_| Error::Transport)??;
    if ready["type"] != "relay.ready" {
        return Err(Error::Protocol);
    }
    // The local server's 10-second hello clock starts only after the remote
    // client has supplied its hello, never while waiting for Internet pairing.
    let hello = timeout(Duration::from_secs(10), text(&mut remote))
        .await
        .map_err(|_| Error::Transport)??;
    if hello["type"] != "hello" {
        return Err(Error::Protocol);
    }
    let mut host = connect(&local.url, local.token.expose_secret(), MAX_MESSAGE).await?;
    send(&mut host, Message::Text(hello.to_string().into())).await?;
    let cancel = CancellationToken::new();
    let (to_host, from_remote) = mpsc::channel(2);
    let (to_remote, from_host) = mpsc::channel(2);
    tokio::join!(
        endpoint(remote, to_host, from_host, cancel.clone()),
        endpoint(host, to_remote, from_remote, cancel)
    );
    Ok(())
}

async fn endpoint(
    socket: Socket,
    peer: mpsc::Sender<Message>,
    mut incoming: mpsc::Receiver<Message>,
    cancel: CancellationToken,
) {
    let (mut sink, mut stream) = socket.split();
    let (pong_tx, mut pong_rx) = mpsc::channel(1);
    let reader = async {
        loop {
            let message = tokio::select! {
                () = cancel.cancelled() => break,
                message = stream.next() => match message { Some(Ok(message)) => message, _ => break },
            };
            match message {
                Message::Text(_) | Message::Binary(_) => {
                    if !matches!(
                        timeout(Duration::from_secs(5), peer.send(message)).await,
                        Ok(Ok(()))
                    ) {
                        break;
                    }
                }
                Message::Ping(bytes) => {
                    if pong_tx.try_send(Message::Pong(bytes)).is_err() {
                        break;
                    }
                }
                Message::Pong(_) => {}
                Message::Close(_) | Message::Frame(_) => break,
            }
        }
        cancel.cancel();
    };
    let writer = async {
        loop {
            let message = tokio::select! {
                biased;
                () = cancel.cancelled() => break,
                Some(pong) = pong_rx.recv() => pong,
                message = incoming.recv() => match message { Some(message) => message, None => break },
            };
            if !matches!(
                timeout(Duration::from_secs(5), sink.send(message)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        cancel.cancel();
        let _ = timeout(Duration::from_secs(1), sink.close()).await;
    };
    tokio::join!(reader, writer);
}
