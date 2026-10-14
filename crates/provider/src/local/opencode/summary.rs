//! Tool-disabled ACP metadata generation with native session deletion on every exit path.
use std::{collections::BTreeMap, time::Duration};

use serde_json::{Value, json};

use super::{OpenCodeClient, config, launcher};
use crate::{
    local::acp_transport::Transport,
    ports::agent_session::{AgentSessionError, AgentSessionSpec},
};

/// Owned auxiliary session; deletion remains scheduled if its caller drops a pending cleanup.
#[derive(Debug)]
pub(super) struct Temporary {
    client: OpenCodeClient,
    cwd: String,
    pub(super) transport: Option<Transport>,
    pub(super) id: Option<String>,
    cleanup_on_drop: bool,
}

impl Temporary {
    /// Own this child in cwd and schedule cleanup if the calling future is dropped.
    pub(super) fn new(client: &OpenCodeClient, cwd: &str, transport: Transport) -> Self {
        Self {
            client: client.clone(),
            cwd: cwd.into(),
            transport: Some(transport),
            id: None,
            cleanup_on_drop: true,
        }
    }

    /// Delete only this temporary native session and reap its child; cleanup failures fail.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        let Some(transport) = self.transport.as_mut() else {
            return Ok(());
        };
        if let Some(id) = &self.id {
            if transport
                .request("session/delete", json!({"sessionId":id}))
                .await
                .is_err()
            {
                let (mut cleanup, _) = launcher::spawn(
                    &self.client,
                    &self.cwd,
                    &domain::agent_runtime::StoredAgentConfig::default(),
                )
                .await?;
                let removed = cleanup
                    .request("session/delete", json!({"sessionId":id}))
                    .await;
                cleanup.close().await?;
                removed?;
            }
            self.id = None;
        }
        transport.close().await?;
        self.transport = None;
        Ok(())
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            return;
        }
        let Some(transport) = self.transport.take() else {
            return;
        };
        let mut pending = Self {
            client: self.client.clone(),
            cwd: self.cwd.clone(),
            transport: Some(transport),
            id: self.id.take(),
            cleanup_on_drop: false,
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(10), pending.close()).await,
                    Ok(Ok(()))
                ) {
                    tracing::warn!("OpenCode ACP auxiliary session cleanup failed");
                }
            });
        }
    }
}

/// Generate bounded metadata in a tool-disabled native session and delete its transcript.
/// Invalid requests reject; native inference, output bounds and cleanup failures propagate.
pub(super) async fn generate(
    client: &OpenCodeClient,
    spec: &AgentSessionSpec,
    prompt: &str,
    schema: &Value,
) -> Result<String, AgentSessionError> {
    config::validate_spec(spec)?;
    if !schema.is_object() || prompt.len() > 192 * 1024 {
        return Err(AgentSessionError::Rejected);
    }
    let mut selected = spec.config.clone();
    selected.mode_id = Some("build".into());
    selected.feature_values = Some(BTreeMap::from([("permission".into(), json!("deny"))]));
    selected.system_prompt = Some(
        "Generate only the requested metadata from the supplied text. Return only JSON.".into(),
    );
    let (transport, capabilities) = launcher::auxiliary(client, &spec.cwd, &selected).await?;
    if !capabilities["sessionCapabilities"]["delete"].is_object() {
        return Err(AgentSessionError::Unavailable);
    }
    let mut temporary = Temporary::new(client, &spec.cwd, transport);
    let result = async {
        let transport = temporary.transport.as_mut().ok_or(AgentSessionError::Failed)?;
        let result = transport.request("session/new", json!({"cwd":spec.cwd,"mcpServers":[]})).await?;
        let id = config::text(&result, "sessionId")?.to_owned();
        temporary.id = Some(id.clone());
        let mut options = config::state(&result)?;
        config::apply(transport, &id, &mut options, &selected).await?;
        let mut answer = String::new();
        let result = transport.request_with_updates("session/prompt", json!({"sessionId":id,
            "prompt":[{"type":"text","text":format!("{prompt}\nReturn only JSON matching this schema: {schema}")}]}), |message| {
            if message["method"] == "session/update" {
                if message["params"]["sessionId"] != id { return Err(AgentSessionError::Failed); }
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk" && update["content"]["type"] == "text" {
                    let text = update["content"]["text"].as_str().ok_or(AgentSessionError::Failed)?;
                    if answer.len().saturating_add(text.len()) > 128 * 1024 { return Err(AgentSessionError::Failed); }
                    answer.push_str(text);
                }
            }
            Ok(())
        }).await?;
        if result["stopReason"] != "end_turn" || answer.is_empty() { return Err(AgentSessionError::Failed); }
        Ok(answer)
    }.await;
    temporary.close().await?;
    result
}
