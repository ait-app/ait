//! Local file downloads streamed as `download.*` messages; the Hub download
//! session frames them.

use std::time::Duration;

use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Bytes;
use url::Url;
use uuid::Uuid;

use crate::protocol::{DownloadAck, DownloadMessage};
use crate::{Error, Local, protocol};

/// Largest binary body message of a download.
pub(crate) const CHUNK: usize = 64 * 1024;

/// Validate `token`, request the local file it authorizes and encode the
/// `download.headers` message announcing it.
///
/// # Errors
/// Rejects malformed tokens, unreachable local endpoints and unsuccessful responses.
pub(crate) async fn prepare(
    local: &Local,
    token: &str,
    visitor: Option<Uuid>,
) -> Result<(String, reqwest::Response), Error> {
    protocol::validate_download_token(token)?;
    let response = local_response(local, token, visitor).await?;
    let headers = protocol::encode(&DownloadMessage::Headers {
        status: response.status().as_u16(),
        content_length: response.content_length(),
        content_type: response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok()),
    })?;
    Ok((headers, response))
}

/// Split one body read into binary messages of at most [`CHUNK`] bytes without copying.
pub(crate) fn chunks(bytes: &Bytes) -> impl Iterator<Item = Bytes> + '_ {
    (0..bytes.len())
        .step_by(CHUNK)
        .map(|start| bytes.slice(start..bytes.len().min(start + CHUNK)))
}

/// Encode the `download.end` message after `bytes` body bytes were sent.
///
/// # Errors
/// Returns a protocol error if serialization fails.
pub(crate) fn end(bytes: u64) -> Result<String, Error> {
    protocol::encode(&DownloadMessage::End { bytes })
}

/// Whether `text` is the destination's final `download.complete` acknowledgement.
pub(crate) fn completed(text: &str) -> bool {
    protocol::decode::<DownloadAck>(text).is_ok()
}

async fn local_response(
    local: &Local,
    token: &str,
    visitor: Option<Uuid>,
) -> Result<reqwest::Response, Error> {
    let mut download = Url::parse(&local.url).map_err(|_| Error::Protocol)?;
    download.set_scheme("http").map_err(|()| Error::Protocol)?;
    download.set_path("/api/files/download");
    download.query_pairs_mut().append_pair("token", token);
    let client = local
        .downloads
        .get_or_try_init(|| async {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .build()
                .map_err(|_| Error::Transport)
        })
        .await?;
    let mut request = client.get(download);
    if let Some(visitor) = visitor {
        request = request.header(model::share::DOWNLOAD_VISITOR_HEADER, visitor.to_string());
    }
    let response = timeout(Duration::from_secs(10), request.send())
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
    if !response.status().is_success() {
        return Err(Error::Protocol);
    }
    Ok(response)
}
