//! Authenticated loopback Host RPC; launch tokens and cookies never enter persistence.
use futures_util::StreamExt;
use reqwest::{Client, Url, header};
use serde_json::{Value, json};
use std::time::Duration;

use crate::ports::agent_session::AgentSessionError;

pub(super) const MAX_FRAME: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Api {
    pub(super) base: Url,
    pub(super) cookie: header::HeaderValue,
    client: Client,
    pub(super) diagnostics: crate::local::diagnostics::Diagnostics,
}

impl std::fmt::Debug for Api {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DshHost")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Api {
    /// Exchange the owned runtime's launch URL for a cookie, rejecting non-loopback targets.
    pub(super) async fn connect(
        launch: &str,
        deadline: Duration,
        diagnostics: crate::local::diagnostics::Diagnostics,
    ) -> Result<Self, AgentSessionError> {
        let mut base = Url::parse(launch).map_err(|_| AgentSessionError::Failed)?;
        if base.scheme() != "http"
            || base.host_str() != Some("127.0.0.1")
            || base.port().is_none()
            || base.path() != "/"
            || !base.username().is_empty()
            || base.password().is_some()
            || base.fragment().is_some()
            || base.query_pairs().count() != 1
            || !base
                .query_pairs()
                .any(|(key, value)| key == "token" && !value.is_empty())
        {
            return Err(AgentSessionError::Failed);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(deadline)
            .build()
            .map_err(|_| AgentSessionError::Failed)?;
        let response = client
            .get(base.clone())
            .send()
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if !response.status().is_redirection() {
            return Err(AgentSessionError::Failed);
        }
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .filter(|value| !value.is_empty())
            .ok_or(AgentSessionError::Failed)?;
        let mut cookie =
            header::HeaderValue::from_str(cookie).map_err(|_| AgentSessionError::Failed)?;
        cookie.set_sensitive(true);
        base.set_query(None);
        Ok(Self {
            base,
            cookie,
            client,
            diagnostics,
        })
    }

    /// Call one native endpoint exactly once and validate its correlated response envelope.
    pub(super) async fn call(
        &self,
        endpoint: &str,
        args: Value,
    ) -> Result<Value, AgentSessionError> {
        self.optional_call(endpoint, args).await?.ok_or_else(|| {
            let mut diagnostic = crate::ports::diagnostics::Diagnostic::new(
                crate::ports::diagnostics::Category::Unknown,
                "http",
            );
            diagnostic.http_status = Some(404);
            self.diagnostics.observe(diagnostic, endpoint);
            AgentSessionError::Failed
        })
    }

    /// Query an optional native endpoint; only HTTP 404 means the Host lacks this capability.
    /// All other transport, HTTP and envelope failures remain errors, with no raw response text.
    pub(super) async fn optional_call(
        &self,
        endpoint: &str,
        args: Value,
    ) -> Result<Option<Value>, AgentSessionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let body =
            json!({"type":"client-request","rpcId":id,"method":endpoint,"payload":{"args":args}});
        if body.to_string().len() > 64 * 1024 * 1024 {
            return Err(AgentSessionError::Rejected);
        }
        let response = self
            .client
            .post(
                self.base
                    .join(&format!("api/{endpoint}"))
                    .map_err(|_| AgentSessionError::Failed)?,
            )
            .header(header::COOKIE, self.cookie.clone())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                let category = if error.is_timeout() {
                    crate::ports::diagnostics::Category::Timeout
                } else {
                    crate::ports::diagnostics::Category::Network
                };
                self.diagnostics.observe(
                    crate::ports::diagnostics::Diagnostic::new(category, "http"),
                    endpoint,
                );
                AgentSessionError::Failed
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let mut error = json!({"statusCode":status});
            self.diagnostics.observe_native(&error, "http", endpoint);
            // Inspect only a bounded JSON error message; never surface a raw response body.
            let mut chunks = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(Ok(chunk)) = chunks.next().await {
                if bytes.len().saturating_add(chunk.len()) > 32 * 1024 {
                    break;
                }
                bytes.extend_from_slice(&chunk);
            }
            if let Ok(body) = serde_json::from_slice::<Value>(&bytes) {
                let reason = body.get("error").unwrap_or(&body);
                error["message"] = reason
                    .as_str()
                    .map_or_else(|| reason["message"].clone(), |message| json!(message));
                error["type"] = reason["type"].clone();
                self.diagnostics.observe_native(&error, "http", endpoint);
            }
            return Err(AgentSessionError::Failed);
        }
        let mut chunks = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| AgentSessionError::Failed)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_FRAME {
                return Err(AgentSessionError::Failed);
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: Value =
            serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?;
        if response["type"] != "server-response" || response["rpcId"] != id {
            return Err(AgentSessionError::Failed);
        }
        match response["result"]["ok"].as_bool() {
            Some(true) => response["result"]
                .get("value")
                .cloned()
                // DSH's event-result acknowledgement is void; JSON omits its undefined value.
                .or_else(|| (endpoint == "$events/result").then_some(Value::Null))
                .map(Some)
                .ok_or(AgentSessionError::Failed),
            Some(false) => {
                self.diagnostics
                    .observe_native(&response["result"]["error"], "rpc", endpoint);
                Err(AgentSessionError::Rejected)
            }
            None => Err(AgentSessionError::Failed),
        }
    }
}

#[cfg(test)]
mod tests;
