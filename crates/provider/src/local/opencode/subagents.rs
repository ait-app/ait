//! ACP child ownership and projections. Native task results are the durable relationship proof.
pub(super) mod history;
pub(super) mod live;

use std::collections::BTreeMap;

use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};

use super::PROVIDER;
use crate::ports::{agent_session::AgentSessionError, controls::NativeSubagent};

const MAX_CHILDREN: usize = 256;
const LOCATOR: &str = "opencodeChild";

/// Bounded native task snapshots retained independently of display preview truncation.
#[derive(Debug, Default)]
pub(super) struct Links {
    pending: BTreeMap<String, Value>,
    pub(super) children: BTreeMap<String, Value>,
}

impl Links {
    /// Merge a tool update and retain only verified structured child identities.
    pub(super) fn update(&mut self, update: &Value) -> Result<Option<Value>, AgentSessionError> {
        if !matches!(
            update["sessionUpdate"].as_str(),
            Some("tool_call" | "tool_call_update")
        ) {
            return Ok(None);
        }
        let call = super::config::text(update, "toolCallId")?;
        if self.pending.len() >= 128 && !self.pending.contains_key(call) {
            return Err(AgentSessionError::Failed);
        }
        let snapshot = self
            .pending
            .entry(call.to_owned())
            .or_insert_with(|| json!({}));
        for key in ["name", "title", "status"] {
            if let Some(value) = update.get(key).and_then(Value::as_str) {
                snapshot[key] = json!(value.chars().take(512).collect::<String>());
            }
        }
        // A large task prompt must not truncate its agent type or ownership information.
        if let Some(input) = update.get("rawInput") {
            for key in ["description", "subagent_type", "agent", "model"] {
                if let Some(value) = input[key].as_str() {
                    snapshot["rawInput"][key] = json!(value.chars().take(512).collect::<String>());
                }
            }
        }
        let metadata = &update["rawOutput"]["metadata"];
        let name = snapshot["name"]
            .as_str()
            .or_else(|| snapshot["title"].as_str());
        let task = matches!(name, Some("task" | "subagent"))
            || (snapshot["name"].is_null() && snapshot["rawInput"]["subagent_type"].is_string());
        let child = if task {
            metadata["sessionId"]
                .as_str()
                .or_else(|| metadata["sessionID"].as_str())
        } else {
            None
        };
        let result = if let Some(id) = child {
            valid_id(id)?;
            if self.children.len() >= MAX_CHILDREN && !self.children.contains_key(id) {
                return Err(AgentSessionError::Failed);
            }
            let input = &snapshot["rawInput"];
            let status = if metadata["background"] == true || metadata["status"] == "running" {
                "running"
            } else if snapshot["status"] == "failed" {
                "failed"
            } else if snapshot["status"] == "completed" {
                "completed"
            } else {
                "running"
            };
            let link = json!({"id":id,"parentID":metadata["parentSessionId"],"toolCallId":call,
                "title":input["description"],
                "agent":input["subagent_type"].as_str().or_else(|| input["agent"].as_str()),
                "model":metadata["model"]["providerID"].as_str().zip(metadata["model"]["modelID"].as_str())
                    .map(|(provider, model)| format!("{provider}/{model}").chars().take(512).collect::<String>())
                    .or_else(|| input["model"].as_str().map(str::to_owned)),"status":status});
            self.children.insert(id.to_owned(), link.clone());
            Some(link)
        } else {
            None
        };
        if matches!(snapshot["status"].as_str(), Some("completed" | "failed")) {
            self.pending.remove(call);
        }
        Ok(result)
    }

    /// Resolve structured task outputs against the native session that emitted them.
    pub(super) fn descriptors(
        &self,
        parent: &str,
        cwd: &str,
    ) -> Result<Vec<NativeSubagent>, AgentSessionError> {
        self.children
            .values()
            .map(|link| descriptor(link, parent, cwd))
            .collect()
    }
}

fn valid_id(id: &str) -> Result<(), AgentSessionError> {
    if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
        return Err(AgentSessionError::Failed);
    }
    Ok(())
}

fn descriptor(link: &Value, parent: &str, cwd: &str) -> Result<NativeSubagent, AgentSessionError> {
    let id = super::config::text(link, "id")?;
    valid_id(parent)?;
    if id == parent
        || link["parentID"]
            .as_str()
            .is_some_and(|declared| declared != parent)
    {
        return Err(AgentSessionError::Failed);
    }
    let now = chrono::Utc::now().to_rfc3339();
    let title = link["title"]
        .as_str()
        .map(|text| text.chars().take(512).collect::<String>());
    let subtitle = [link["agent"].as_str(), link["model"].as_str()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    Ok(NativeSubagent {
        id: id.to_owned(),
        parent_id: parent.to_owned(),
        cwd: cwd.to_owned(),
        persistence: Some(AgentPersistenceHandle {
            provider: PROVIDER.into(),
            session_id: id.to_owned(),
            native_handle: Some(json!(id)),
            metadata: Some(BTreeMap::from([(
                LOCATOR.into(),
                json!({"parent":parent,"cwd":cwd}),
            )])),
        }),
        descriptor: json!({"id":id,"provider":PROVIDER,"title":title,
            "description":title,"status":link["status"],"createdAt":now,"updatedAt":now,
            "toolCallId":link["toolCallId"],"cwd":cwd,"subtitle":(!subtitle.is_empty()).then_some(subtitle)}),
    })
}

#[cfg(test)]
mod tests;
