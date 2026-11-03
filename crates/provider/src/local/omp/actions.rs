use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::ports::agent_session::{AgentSessionError, AgentSessionFuture};

type Receipt = oneshot::Sender<Result<Value, AgentSessionError>>;

/// In-flight native controls, independent of foreground prompts and callback delivery.
#[derive(Debug, Default)]
pub(super) struct Pending {
    requests: BTreeMap<String, (String, Receipt)>,
    refreshes: VecDeque<Refresh>,
    ready: Vec<Refresh>,
}

/// A native history mutation awaiting host projection and identity reconciliation.
#[derive(Debug)]
pub(super) struct Refresh {
    pub(super) action: String,
    sender: Receipt,
    result: Value,
}

impl Refresh {
    /// Resolve the receipt only after the replacement has been prepared for the owner.
    pub(super) fn finish(self, result: Result<(), AgentSessionError>) {
        let _ = self.sender.send(result.map(|()| self.result));
    }
}

/// Whether an action changes the native conversation or active tree leaf.
pub(super) fn changes_history(action: &str) -> bool {
    matches!(
        action,
        "new_session" | "branch" | "fork" | "navigate_tree" | "handoff"
    )
}

impl Pending {
    /// Reject excess operations before writing anything to the native process.
    pub(super) fn check_capacity(&self) -> Result<(), AgentSessionError> {
        if self.requests.len() + self.refreshes.len() + self.ready.len() >= 8 {
            return Err(AgentSessionError::Busy);
        }
        Ok(())
    }

    /// Correlate a submitted control with its detached completion receipt.
    pub(super) fn insert(
        &mut self,
        id: String,
        action: &str,
    ) -> AgentSessionFuture<'static, Value> {
        let (sender, receiver) = oneshot::channel();
        self.requests.insert(id, (action.to_owned(), sender));
        Box::pin(async move {
            tokio::time::timeout(std::time::Duration::from_mins(11), receiver)
                .await
                .map_err(|_| AgentSessionError::Failed)?
                .map_err(|_| AgentSessionError::Failed)?
        })
    }

    /// Resolve only an owned response; native rejection does not close a usable connection.
    pub(super) fn capture(&mut self, event: &Value) -> bool {
        let Some(id) = event["id"].as_str() else {
            return false;
        };
        let Some((action, sender)) = self.requests.remove(id) else {
            return false;
        };
        let result = if event["command"] != action {
            Err(AgentSessionError::Failed)
        } else if event["success"] != true {
            let safe = super::bridge::redact_event(event);
            Ok(
                json!({"text":"","error":safe["error"].as_str().unwrap_or("Native operation rejected")}),
            )
        } else {
            let mut result = json!({"text":display(&event["data"])});
            if action == "export_html"
                && let Some(path) = event["data"]["path"].as_str()
                && let Some(name) = std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                && name.starts_with("ait-export-")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
            {
                result["file"] = json!({"path":name,"fileName":name});
            }
            Ok(result)
        };
        match result {
            Ok(result)
                if event["success"] == true
                    && changes_history(&action)
                    && event["data"]["cancelled"] != true =>
            {
                self.refreshes.push_back(Refresh {
                    action,
                    sender,
                    result,
                });
            }
            result => {
                let _ = sender.send(result);
            }
        }
        true
    }

    /// Take one native history change that must be reconciled before accepting another input.
    pub(super) fn take_refresh(&mut self) -> Option<Refresh> {
        self.refreshes.pop_front()
    }

    /// Hold a prepared native replacement until the host confirms durable observation.
    pub(super) fn prepared(&mut self, refresh: Refresh) {
        self.ready.push(refresh);
    }

    /// Release successful history controls only after the owner commits their observations.
    pub(super) fn committed(&mut self) {
        for refresh in self.ready.drain(..) {
            refresh.finish(Ok(()));
        }
    }

    /// Prevent overlapping session changes and new input until native identity is reconciled.
    pub(super) fn changing_history(&self) -> bool {
        !self.refreshes.is_empty()
            || !self.ready.is_empty()
            || self
                .requests
                .values()
                .any(|(action, _)| changes_history(action))
    }

    /// Release detached waiters when their native writer is closed.
    pub(super) fn close(&mut self) {
        for (_, (_, sender)) in std::mem::take(&mut self.requests) {
            let _ = sender.send(Err(AgentSessionError::Failed));
        }
        for refresh in self.refreshes.drain(..) {
            refresh.finish(Err(AgentSessionError::Failed));
        }
        for refresh in self.ready.drain(..) {
            refresh.finish(Err(AgentSessionError::Failed));
        }
    }
}

fn field(id: &str, label: &str, options: &[&str], required: bool) -> Value {
    json!({"id":id,"label":label,"options":options,"required":required})
}

fn action(id: &str, label: &str, fields: Vec<Value>) -> Value {
    let fields = Value::Array(fields);
    json!({"id":id,"label":label,"fields":fields})
}

/// User-facing operations backed by exported official RPCs; session ownership stays with Ait.
pub(super) fn catalog() -> Value {
    static CATALOG: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let mut actions = session_actions();
            actions.extend(account_actions());
            actions.extend(runtime_actions());
            json!(actions)
        })
        .clone()
}

fn session_actions() -> Vec<Value> {
    vec![
        action("new_session", "Start a new native conversation", vec![]),
        action(
            "branch",
            "Branch from an entry",
            vec![field("entryId", "Entry ID", &[], true)],
        ),
        action(
            "fork",
            "Fork native conversation",
            vec![field("entryId", "Entry ID (optional)", &[], false)],
        ),
        action(
            "navigate_tree",
            "Navigate to a session entry",
            vec![
                field("entryId", "Entry ID", &[], true),
                field(
                    "summarize",
                    "Summarize the previous branch",
                    &["true", "false"],
                    false,
                ),
                field("customInstructions", "Summary instructions", &[], false),
            ],
        ),
        action(
            "handoff",
            "Hand off conversation",
            vec![field("customInstructions", "Instructions", &[], false)],
        ),
        action("get_session_stats", "Session statistics", vec![]),
        action("get_tree", "Session tree", vec![]),
        action("abort_branch_summary", "Stop branch summary", vec![]),
        action("get_entries", "Session entries", vec![]),
        action("get_branch_messages", "Branch points", vec![]),
        action(
            "goal",
            "Native goal",
            vec![
                field(
                    "op",
                    "Operation",
                    &["get", "create", "resume", "pause", "drop"],
                    true,
                ),
                field("objective", "Objective (create)", &[], false),
                field(
                    "token_budget",
                    "Token budget (create, optional)",
                    &[],
                    false,
                ),
            ],
        ),
        action(
            "set_label",
            "Label a session entry",
            vec![
                field("entryId", "Entry ID", &[], true),
                field("label", "Label (empty removes it)", &[], false),
            ],
        ),
        action(
            "set_session_name",
            "Rename native session",
            vec![field("name", "Name", &[], true)],
        ),
        action("export_html", "Export conversation as HTML", vec![]),
    ]
}

fn account_actions() -> Vec<Value> {
    let provider = || field("providerId", "Provider", &[], true);
    let child = || field("subagentId", "Subagent ID", &[], true);
    vec![
        action("get_login_providers", "Login providers", vec![]),
        action("login", "Log in to a provider", vec![provider()]),
        action(
            "get_logout_accounts",
            "Saved provider accounts",
            vec![provider()],
        ),
        action(
            "logout",
            "Log out a saved account",
            vec![provider(), field("credentialId", "Account ID", &[], true)],
        ),
        action("get_subagents", "Native subagents", vec![]),
        action("cancel_subagent", "Cancel a subagent", vec![child()]),
        action(
            "steer_subagent",
            "Send guidance to a subagent",
            vec![child(), field("message", "Guidance", &[], true)],
        ),
        action("get_btw_history", "Side question history", vec![]),
        action(
            "btw",
            "Ask a side question",
            vec![field("question", "Question", &[], true)],
        ),
        action(
            "btw_cancel",
            "Cancel a side question",
            vec![field("recordId", "Question ID", &[], true)],
        ),
    ]
}

fn runtime_actions() -> Vec<Value> {
    let mode = || field("mode", "Delivery", &["all", "one-at-a-time"], true);
    vec![
        action("set_steering_mode", "Steering delivery", vec![mode()]),
        action("set_follow_up_mode", "Follow-up delivery", vec![mode()]),
        action(
            "set_interrupt_mode",
            "Interrupt delivery",
            vec![field("mode", "Delivery", &["immediate", "wait"], true)],
        ),
        action(
            "set_cache_warming",
            "Cache warming",
            vec![field("mode", "Mode", &["off", "streaming", "idle"], true)],
        ),
        action("abort_retry", "Stop retrying", vec![]),
        action("abort_bash", "Stop native shell command", vec![]),
        action(
            "bash",
            "Run native shell command",
            vec![field("command", "Command", &[], true)],
        ),
        action(
            "live_start",
            "Start native voice on the host",
            vec![
                field("voice", "Voice", &[], false),
                field("instructions", "Instructions", &[], false),
            ],
        ),
        action("live_stop", "Stop native voice", vec![]),
        action(
            "live_mute",
            "Mute native voice",
            vec![field("muted", "Muted", &["true", "false"], true)],
        ),
        action("dictation_start", "Start dictation on the host", vec![]),
        action("dictation_stop", "Stop native dictation", vec![]),
    ]
}

/// Validate an advertised operation and construct only its declared native arguments.
/// Returns rejection before any side effect for unknown fields or malformed values.
pub(super) fn validate(
    action: &str,
    arguments: &Value,
    cwd: &str,
) -> Result<Value, AgentSessionError> {
    let catalog = catalog();
    let spec = catalog
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == action))
        .ok_or(AgentSessionError::Rejected)?;
    let fields = spec["fields"].as_array().ok_or(AgentSessionError::Failed)?;
    let values = arguments
        .as_object()
        .filter(|values| values.len() <= fields.len())
        .ok_or(AgentSessionError::Rejected)?;
    if values
        .keys()
        .any(|key| !fields.iter().any(|field| field["id"] == key.as_str()))
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut result = json!({});
    for field in fields {
        let key = field["id"].as_str().ok_or(AgentSessionError::Failed)?;
        let value = values
            .get(key)
            .map(|value| value.as_str().ok_or(AgentSessionError::Rejected))
            .transpose()?
            .unwrap_or_default();
        if value.len() > 16384
            || value.contains('\0')
            || (field["required"] == true && value.trim().is_empty())
        {
            return Err(AgentSessionError::Rejected);
        }
        if value.is_empty() {
            continue;
        }
        if let Some(options) = field["options"]
            .as_array()
            .filter(|options| !options.is_empty())
            && !options.iter().any(|option| option == value)
        {
            return Err(AgentSessionError::Rejected);
        }
        result[key] = match key {
            "muted" | "summarize" => json!(value == "true"),
            "credentialId" => json!(
                value
                    .parse::<u64>()
                    .map_err(|_| AgentSessionError::Rejected)?
            ),
            "token_budget" => json!(
                value
                    .parse::<u64>()
                    .ok()
                    .filter(|budget| (1..=9_007_199_254_740_991).contains(budget))
                    .ok_or(AgentSessionError::Rejected)?
            ),
            _ => json!(value),
        };
    }
    if action == "goal" {
        if result["op"] == "create" {
            if result["objective"]
                .as_str()
                .is_none_or(|text| text.trim().is_empty())
            {
                return Err(AgentSessionError::Rejected);
            }
        } else if result.get("objective").is_some() || result.get("token_budget").is_some() {
            return Err(AgentSessionError::Rejected);
        }
    }
    if action == "export_html" {
        result["outputPath"] = json!(
            std::path::Path::new(cwd).join(format!("ait-export-{}.html", uuid::Uuid::new_v4()))
        );
    } else if action == "dictation_start" {
        result["captureId"] = json!(uuid::Uuid::new_v4().to_string());
    }
    Ok(result)
}

fn display(value: &Value) -> String {
    let text = if value.is_null() {
        "Done".to_owned()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_default()
    };
    text.chars().take(32768).collect()
}

/// Require membership in this native owner's registry before controlling a descendant.
pub(super) fn validate_child(owned: &Value, id: &Value) -> Result<(), AgentSessionError> {
    if !owned["subagents"]
        .as_array()
        .is_some_and(|children| children.iter().any(|child| child["id"] == *id))
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
