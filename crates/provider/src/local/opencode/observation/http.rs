//! Read-only access to the HTTP endpoint belonging to our ACP child.
use std::{net::TcpListener, time::Duration};

use base64::Engine;
use reqwest::{Client as HttpClient, RequestBuilder, Response, Url, header};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use tokio::process::Command;

use crate::ports::agent_session::AgentSessionError;

const MAX_BODY: usize = 32 * 1024 * 1024;
const MAX_FRAME: usize = 8 * 1024 * 1024;

/// A loopback endpoint protected by credentials private to one owned ACP process.
#[derive(Clone, Debug)]
pub(super) struct Client {
    http: HttpClient,
    base: Url,
    pub(super) deadline: Duration,
}

/// Reserve the address until immediately before spawning its authenticated owner.
#[derive(Debug)]
pub(super) struct Prepared {
    listener: TcpListener,
    password: SecretString,
    pub(super) client: Client,
}

impl Prepared {
    /// Prepare an authenticated loopback endpoint without connecting to an existing process.
    pub(super) fn new(deadline: Duration) -> Result<Self, AgentSessionError> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| AgentSessionError::Failed)?;
        let address = listener
            .local_addr()
            .map_err(|_| AgentSessionError::Failed)?;
        let password = SecretString::from(uuid::Uuid::new_v4().to_string());
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("opencode:{}", password.expose_secret()));
        let mut authorization = header::HeaderValue::from_str(&format!("Basic {encoded}"))
            .map_err(|_| AgentSessionError::Failed)?;
        authorization.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization);
        let client = HttpClient::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(deadline)
            .default_headers(headers)
            .build()
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(Self {
            listener,
            password,
            client: Client {
                http: client,
                base: Url::parse(&format!("http://{address}/"))
                    .map_err(|_| AgentSessionError::Failed)?,
                deadline,
            },
        })
    }

    /// Configure only this ACP child's server; secrets never enter argv or persisted handles.
    pub(super) fn configure(&self, command: &mut Command) -> Result<(), AgentSessionError> {
        let port = self
            .listener
            .local_addr()
            .map_err(|_| AgentSessionError::Failed)?
            .port();
        command
            .args([
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--mdns=false",
            ])
            .env("OPENCODE_SERVER_USERNAME", "opencode")
            .env("OPENCODE_SERVER_PASSWORD", self.password.expose_secret());
        Ok(())
    }
}

impl Client {
    /// Build a GET against this owned origin; arbitrary URLs and mutating requests are unavailable.
    pub(super) fn get(&self, path: &str, cwd: &str) -> RequestBuilder {
        let mut url = self.base.clone();
        url.set_path(path);
        self.http.get(url).query(&[("directory", cwd)])
    }

    /// Read bounded JSON and an opaque pagination cursor within one control-operation deadline.
    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<(T, Option<String>), AgentSessionError> {
        let mut response = request
            .timeout(self.deadline)
            .send()
            .await
            .and_then(Response::error_for_status)
            .map_err(|_| AgentSessionError::Failed)?;
        let cursor = response
            .headers()
            .get("x-next-cursor")
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| AgentSessionError::Failed)?;
        if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 4096)
        {
            return Err(AgentSessionError::Failed);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AgentSessionError::Failed)?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err(AgentSessionError::Failed);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((
            serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?,
            cursor,
        ))
    }

    /// Establish the native SSE stream without imposing a duration limit on model execution.
    pub(super) async fn events(&self, cwd: &str) -> Result<Events, AgentSessionError> {
        let response = tokio::time::timeout(self.deadline, self.get("/event", cwd).send())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .and_then(Response::error_for_status)
            .map_err(|_| AgentSessionError::Failed)?;
        if !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(Events {
            response,
            pending: Vec::new(),
            data: Vec::new(),
        })
    }
}

/// Incremental, bounded SSE decoder; partially read frames survive cancellation of `next`.
pub(super) struct Events {
    response: Response,
    pending: Vec<u8>,
    data: Vec<u8>,
}

impl Events {
    /// Read the next native JSON event, accepting CRLF, comments and split network frames.
    pub(super) async fn next(&mut self) -> Result<serde_json::Value, AgentSessionError> {
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<_> = self.pending.drain(..=end).collect();
                let line = line.strip_suffix(b"\n").unwrap_or(&line);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                if line.is_empty() && !self.data.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    return serde_json::from_slice(&data).map_err(|_| AgentSessionError::Failed);
                }
                if let Some(data) = line.strip_prefix(b"data:") {
                    let data = data.strip_prefix(b" ").unwrap_or(data);
                    if !self.data.is_empty() {
                        self.data.push(b'\n');
                    }
                    if self.data.len().saturating_add(data.len()) > MAX_FRAME {
                        return Err(AgentSessionError::Failed);
                    }
                    self.data.extend_from_slice(data);
                }
                continue;
            }
            let chunk = self
                .response
                .chunk()
                .await
                .map_err(|_| AgentSessionError::Failed)?
                .ok_or(AgentSessionError::Failed)?;
            if self.pending.len().saturating_add(chunk.len()) > MAX_FRAME {
                return Err(AgentSessionError::Failed);
            }
            self.pending.extend_from_slice(&chunk);
        }
    }
}
