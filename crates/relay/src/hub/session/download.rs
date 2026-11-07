//! A Hub download session (`open{k:"download"}`), speaking the Relay download
//! messages: `download.headers`, binary chunks and `download.end` to the peer,
//! then `download.complete` back once the destination's final write finished.

use std::time::Duration;

use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Bytes;

use super::{Frame, Outlet, Output, PayloadKind, Session};
use crate::download;

/// A stalled local body read ends the download.
const BODY_READ: Duration = Duration::from_secs(30);
/// The destination acknowledges after its final write and file synchronization.
const COMPLETION: Duration = Duration::from_secs(10);

/// The `open` payload of a download session.
#[derive(Deserialize)]
struct Request {
    /// Local one-use download token issued over RPC; bridged legacy downloads
    /// name it `download_token`.
    #[serde(alias = "download_token")]
    token: SecretString,
}

/// Stream the local file authorized by the request token, then wait for the
/// peer's completion. A guest download names its visitor so only a token
/// issued to that visitor is served.
///
/// # Errors
/// Returns the session close code: `invalid_message` for a malformed request or
/// peer message, `unavailable` when the local file cannot be served, `timeout`
/// without a completion, and `cancelled` when the peer went away.
pub(super) async fn serve(session: &mut Session, request: &[u8]) -> Result<(), &'static str> {
    let request: Request = serde_json::from_slice(request).map_err(|_| "invalid_message")?;
    let visitor = session.visitor.as_ref().map(|visitor| visitor.user_id);
    let (headers, response) =
        download::prepare(&session.local, request.token.expose_secret(), visitor)
            .await
            .map_err(|_| "unavailable")?;
    if session
        .output
        .send((session.id, Output::Accept(Bytes::new())))
        .await
        .is_err()
    {
        return Err("cancelled");
    }
    let (complete_tx, complete_rx) = oneshot::channel();
    let inbound = &mut session.inbound;
    let reader = async {
        let mut complete = Some(complete_tx);
        loop {
            match inbound.recv().await {
                Some(Frame::Text(text))
                    if std::str::from_utf8(&text).is_ok_and(download::completed) =>
                {
                    if let Some(complete) = complete.take() {
                        let _ = complete.send(());
                    }
                }
                Some(_) => return "invalid_message",
                None => return "cancelled",
            }
        }
    };
    let mut outlet = Outlet {
        id: session.id,
        output: &session.output,
        credit: session.credit.as_mut(),
    };
    let writer = async {
        outlet.send(PayloadKind::Text, headers.into()).await?;
        let mut body = response.bytes_stream();
        let mut written = 0_u64;
        while let Some(read) = timeout(BODY_READ, body.next())
            .await
            .map_err(|_| "unavailable")?
        {
            let bytes = read.map_err(|_| "unavailable")?;
            for chunk in download::chunks(&bytes) {
                let length = chunk.len() as u64;
                outlet.send(PayloadKind::Binary, chunk).await?;
                written += length;
            }
        }
        let end = download::end(written).map_err(|_| "unavailable")?;
        outlet.send(PayloadKind::Text, end.into()).await?;
        timeout(COMPLETION, complete_rx)
            .await
            .map_err(|_| "timeout")?
            .map_err(|_| "cancelled")
    };
    tokio::select! {
        code = reader => Err(code),
        result = writer => result,
    }
}

#[cfg(test)]
mod tests;
