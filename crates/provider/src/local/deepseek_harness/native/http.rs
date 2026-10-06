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
        })
    }

    /// Call one native endpoint exactly once and validate its correlated response envelope.
    pub(super) async fn call(
        &self,
        endpoint: &str,
        args: Value,
    ) -> Result<Value, AgentSessionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let body =
            json!({"type":"client-request","rpcId":id,"method":endpoint,"payload":{"args":args}});
        if body.to_string().len() > MAX_FRAME {
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
            .map_err(|_| AgentSessionError::Failed)?;
        if !response.status().is_success() {
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
            Some(true) => Ok(response["result"]["value"].clone()),
            Some(false) => Err(AgentSessionError::Rejected),
            None => Err(AgentSessionError::Failed),
        }
    }
}
