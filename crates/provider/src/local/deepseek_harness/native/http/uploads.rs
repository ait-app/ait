//! Bounded-memory native file intake; bytes never pass through a JSON/base64 buffer.

use futures_util::{StreamExt, stream};
use reqwest::header;
use serde_json::Value;
use tokio::{fs::File, io::AsyncReadExt};

use super::{Api, MAX_FRAME};
use crate::ports::agent_session::AgentSessionError;

impl Api {
    /// Stage `file` under the exact native `agent`, checking its expected byte length.
    /// Returns the Host's opaque receipt, or a safe native/transport/size error.
    pub(in crate::local::deepseek_harness::native) async fn upload_file(
        &self,
        agent: &str,
        name: &str,
        file: File,
        bytes: u64,
    ) -> Result<String, AgentSessionError> {
        let mut url = self
            .base
            .join("api/session/uploadFileBinary")
            .map_err(|_| AgentSessionError::Failed)?;
        url.query_pairs_mut()
            .append_pair("sessionId", agent)
            .append_pair("name", name);
        let body = stream::try_unfold((file, bytes), |(mut file, remaining)| async move {
            let mut chunk = vec![0; (remaining.saturating_add(1).min(64 * 1024)) as usize];
            let read = file.read(&mut chunk).await?;
            if read as u64 > remaining || read == 0 && remaining != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "upload size changed",
                ));
            }
            if read == 0 {
                return Ok(None);
            }
            chunk.truncate(read);
            Ok(Some((chunk, (file, remaining - read as u64))))
        });
        let response = self
            .client
            .post(url)
            .header(header::COOKIE, self.cookie.clone())
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(reqwest::Body::wrap_stream(body))
            .send()
            .await
            .map_err(|error| self.transport_error(&error, "file_upload", "http_send"))?;
        if !response.status().is_success() {
            return Err(AgentSessionError::Failed);
        }
        let mut chunks = response.bytes_stream();
        let mut encoded = Vec::new();
        while let Some(chunk) = chunks.next().await {
            let chunk =
                chunk.map_err(|error| self.transport_error(&error, "file_upload", "http_body"))?;
            if encoded.len().saturating_add(chunk.len()) > MAX_FRAME {
                return Err(AgentSessionError::Failed);
            }
            encoded.extend_from_slice(&chunk);
        }
        let response: Value =
            serde_json::from_slice(&encoded).map_err(|_| AgentSessionError::Failed)?;
        if response["ok"] == false {
            return Err(
                if response["error"]["code"] == "session/attachment-invalid" {
                    AgentSessionError::Rejected
                } else {
                    AgentSessionError::Failed
                },
            );
        }
        if response["ok"] != true || response["value"]["file"]["bytes"].as_u64() != Some(bytes) {
            return Err(AgentSessionError::Failed);
        }
        response["value"]["receiptId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .map(str::to_owned)
            .ok_or(AgentSessionError::Failed)
    }
}
