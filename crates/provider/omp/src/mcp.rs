//! Session-private Ait MCP tools, executed through OMP's public host-tool RPC.
//! Credentials stay in memory; workers never inherit their parent's caller identity.

use std::collections::BTreeMap;
use std::time::Duration;

use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::AgentSessionError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};
use tokio_util::task::AbortOnDropHandle;

use crate::transport::{Transport, Writer};

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_CALLS: usize = 16;
const PREFIX: &str = "mcp__ait__";

/// Only the Ait stateless HTTP endpoint is supported; other MCP transports remain native-owned.
pub(super) fn valid(config: &StoredAgentConfig) -> bool {
    config.mcp_servers.as_ref().is_none_or(|servers| {
        servers.iter().all(|(name, server)| {
            name == "ait" && server["type"] == "http" && connection(server).is_ok()
        })
    })
}

fn connection(server: &Value) -> Result<(reqwest::Url, HeaderMap), AgentSessionError> {
    let url = server["url"]
        .as_str()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .filter(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
        .ok_or(AgentSessionError::Rejected)?;
    let mut headers = HeaderMap::new();
    if let Some(values) = server.get("headers") {
        let values = values
            .as_object()
            .filter(|values| values.len() <= 64)
            .ok_or(AgentSessionError::Rejected)?;
        for (name, value) in values {
            let name = HeaderName::try_from(name).map_err(|_| AgentSessionError::Rejected)?;
            let mut value =
                HeaderValue::try_from(value.as_str().ok_or(AgentSessionError::Rejected)?)
                    .map_err(|_| AgentSessionError::Rejected)?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
    }
    headers.insert(
        "accept",
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        "mcp-protocol-version",
        HeaderValue::from_static("2025-11-25"),
    );
    Ok((url, headers))
}

#[derive(Debug, Clone)]
struct Connection {
    client: reqwest::Client,
    url: reqwest::Url,
}

impl Connection {
    async fn request(&self, method: &str, params: Value) -> Result<Value, AgentSessionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut response = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .send()
            .await
            .map_err(|_| AgentSessionError::Unavailable)?;
        if !response.status().is_success() {
            return Err(AgentSessionError::Rejected);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AgentSessionError::Failed)?
        {
            if body.len() + chunk.len() > MAX_BYTES {
                return Err(AgentSessionError::Failed);
            }
            body.extend_from_slice(&chunk);
        }
        let response: Value =
            serde_json::from_slice(&body).map_err(|_| AgentSessionError::Failed)?;
        if response["id"] != id || response.get("error").is_some() {
            return Err(AgentSessionError::Rejected);
        }
        response
            .get("result")
            .cloned()
            .ok_or(AgentSessionError::Failed)
    }
}

/// A bounded tool catalog and in-flight calls for one parent session.
#[derive(Debug)]
pub(super) struct Mcp {
    connection: Connection,
    definitions: Vec<Value>,
    calls: BTreeMap<String, AbortOnDropHandle<()>>,
    replies: Vec<Value>,
    delivery: Option<AbortOnDropHandle<()>>,
}

impl Mcp {
    /// Connect and register Ait tools before accepting input; never persist the connection.
    /// A refused or malformed catalog fails opening instead of silently losing delegation.
    pub(super) async fn open(
        config: &StoredAgentConfig,
        transport: &mut Transport,
    ) -> Result<Option<Self>, AgentSessionError> {
        let Some(server) = config
            .mcp_servers
            .as_ref()
            .and_then(|servers| servers.get("ait"))
        else {
            return Ok(None);
        };
        let (url, headers) = connection(server)?;
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(65))
            .build()
            .map_err(|_| AgentSessionError::Failed)?;
        let connection = Connection { client, url };
        connection.request("initialize", json!({"protocolVersion":"2025-11-25", "capabilities":{}, "clientInfo":{"name":"ait-omp", "version":env!("CARGO_PKG_VERSION")}})).await?;
        let catalog = connection.request("tools/list", json!({})).await?;
        let tools = catalog["tools"]
            .as_array()
            .filter(|tools| tools.len() <= 512)
            .ok_or(AgentSessionError::Failed)?;
        if catalog.get("nextCursor").is_some() {
            return Err(AgentSessionError::Unavailable);
        }
        let definitions: Vec<_> = tools.iter().map(|tool| {
            let name = tool["name"].as_str().filter(|name| !name.is_empty() && name.len() <= 128 && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
                .ok_or(AgentSessionError::Failed)?;
            if !tool["inputSchema"].is_object() { return Err(AgentSessionError::Failed); }
            Ok(json!({"name":format!("{PREFIX}{name}"), "description":tool["description"], "parameters":tool["inputSchema"], "loadMode":"essential"}))
        }).collect::<Result<_, AgentSessionError>>()?;
        let registered = transport
            .request("set_host_tools", json!({"tools":definitions}))
            .await?;
        if !registered["toolNames"]
            .as_array()
            .is_some_and(|names| definitions.iter().all(|tool| names.contains(&tool["name"])))
        {
            return Err(AgentSessionError::Unavailable);
        }
        Ok(Some(Self {
            connection,
            definitions,
            calls: BTreeMap::new(),
            replies: Vec::new(),
            delivery: None,
        }))
    }

    /// Definitions retained while Vibe adds or removes its five host tools.
    pub(super) fn definitions(&self) -> &[Value] {
        &self.definitions
    }

    /// Stop reading native frames when the bounded rejection queue is full.
    pub(super) fn backpressured(&self) -> bool {
        self.replies.len() >= 64
    }

    /// Flush bounded capacity errors through the session's single ordered writer.
    pub(super) fn poll(&mut self, writer: Writer) {
        self.calls.retain(|_, task| !task.is_finished());
        if self.replies.is_empty()
            || self
                .delivery
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let replies = std::mem::take(&mut self.replies);
        self.delivery = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            for reply in replies {
                if writer.send(&reply).await.is_err() {
                    break;
                }
            }
        })));
    }

    /// Execute only a catalog-owned call, preserving the parent's scoped HTTP headers.
    pub(super) fn consume(
        &mut self,
        event: &Value,
        writer: Writer,
    ) -> Result<bool, AgentSessionError> {
        self.calls.retain(|_, task| !task.is_finished());
        if event["type"] == "host_tool_cancel" {
            return Ok(event["targetId"]
                .as_str()
                .is_some_and(|id| self.calls.remove(id).is_some()));
        }
        let Some(name) = event["toolName"]
            .as_str()
            .filter(|name| self.definitions.iter().any(|tool| tool["name"] == *name))
        else {
            return Ok(false);
        };
        let name = name
            .strip_prefix(PREFIX)
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        let id = event["id"]
            .as_str()
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        if self.calls.contains_key(&id) {
            return Ok(true);
        }
        if self.calls.len() >= MAX_CALLS {
            self.replies.push(json!({"type":"host_tool_result","id":id,"isError":true,
                "result":{"content":[{"type":"text","text":"Ait orchestration capacity reached. Retry after active calls finish."}],"isError":true}}));
            return Ok(true);
        }
        let connection = self.connection.clone();
        let arguments = event["arguments"].clone();
        let reply_id = id.clone();
        self.calls.insert(id, AbortOnDropHandle::new(tokio::spawn(async move {
            let result = connection.request("tools/call", json!({"name":name,"arguments":arguments})).await
                .unwrap_or_else(|_| json!({"content":[{"type":"text","text":"Ait orchestration request failed; check the current state before retrying."}],"isError":true}));
            let _ = writer.send(&json!({"type":"host_tool_result", "id":reply_id, "isError":result["isError"] == true, "result":result})).await;
        })));
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
