//! Host event projection and notification preparation, without device tokens or foreground policy.

use domain::session::protocol::SessionEventKind;
use serde_json::{Value, json};
use uuid::Uuid;

use super::sources::Sources;

#[derive(Debug)]
pub(super) struct Event {
    pub(super) method: String,
    pub(super) params: Value,
}

impl Event {
    pub(super) fn project(method: &str, params: &Value) -> Option<Self> {
        let (method, params) = match method {
            "agent.create.update" | "workspace.create.update" => (
                "creation.update",
                json!({
                    "kind": params["kind"],
                    "idempotencyKey": params["idempotencyKey"],
                    "revision": params["revision"],
                    "phase": params["phase"],
                    "agentId": params["agentId"],
                    "workspaceId": params["workspaceId"],
                    "outcomeUnknown": params["outcomeUnknown"],
                    "error": params["error"]
                }),
            ),
            // Publish committed markers; clients fetch actual history from its Host.
            "agent_stream" if params["seq"].is_u64() && params["epoch"].is_string() => (
                "agent.timeline.update",
                json!({"agentId":params["agentId"],"epoch":params["epoch"],"seq":params["seq"]}),
            ),
            "agent.timeline.replacement" => (
                "agent.timeline.update",
                json!({"agentId":params["agentId"],"epoch":params["epoch"],"reset":true}),
            ),
            "agent_attention_required"
            | "terminal_attention_required"
            | "agent_permission_request"
            | "agent_permission_resolved"
            | "creation.update"
            | "workspace_setup_status"
            | "status.server_info" => (method, params.clone()),
            _ => return None,
        };
        if !params.is_object() || serde_json::to_vec(&params).ok()?.len() > 64 * 1024 {
            return None;
        }
        Some(Self {
            method: method.to_owned(),
            params,
        })
    }

    pub(super) fn publication(
        &self,
        sources: &Sources,
        server: &str,
        native: bool,
    ) -> anyhow::Result<Value> {
        let resource = self.params["agentId"]
            .as_str()
            .or_else(|| self.params["terminalId"].as_str());
        let mut publication =
            json!({"eventId":Uuid::new_v4(),"method":self.method,"params":self.params});
        if let Some(resource) = resource {
            publication["resourceId"] = json!(resource);
        }
        if !native {
            return Ok(publication);
        }
        let kind = match self.method.as_str() {
            "agent_attention_required" => SessionEventKind::AgentAttention,
            "terminal_attention_required" => SessionEventKind::TerminalAttention,
            _ => return Ok(publication),
        };
        if let Some(notification) = metadata::push::delivery::prepare_notification(
            sources.agents.as_ref(),
            kind,
            &self.params,
            server,
        )
        .map_err(|_| anyhow::anyhow!("prepare Hub attention notification"))?
        {
            // Center injects source identifiers. Routing data excludes credentials.
            let data = json!({
                "agentId": notification.data["agentId"],
                "terminalId": notification.data["terminalId"],
                "workspaceId": notification.data["workspaceId"],
                "reason": notification.data["reason"]
            });
            publication["notification"] = json!({
                "title": truncate(&notification.title, 200),
                "body": truncate(&notification.body, 1000),
                "data": data
            });
        }
        Ok(publication)
    }

    pub(super) fn attention(&self) -> bool {
        matches!(
            self.method.as_str(),
            "agent_attention_required" | "terminal_attention_required"
        )
    }

    pub(super) fn current_notification(
        publication: &Value,
        sources: &Sources,
        server: &str,
    ) -> anyhow::Result<bool> {
        if !publication["notification"].is_object()
            || publication["method"] != "agent_attention_required"
        {
            return Ok(true);
        }
        metadata::push::delivery::prepare_notification(
            sources.agents.as_ref(),
            SessionEventKind::AgentAttention,
            &publication["params"],
            server,
        )
        .map(|notification| notification.is_some())
        .map_err(|_| anyhow::anyhow!("revalidate pending Hub attention notification"))
    }
}

fn truncate(value: &str, bytes: usize) -> &str {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests;
