//! Display metadata for Codex's current sub-agent activity items.

use serde_json::{Value, json};

/// Project a valid native activity into a labeled, linked sub-agent display item.
pub(super) fn activity(native: &Value) -> Option<Value> {
    let child = native["agentThreadId"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    if !matches!(
        native["kind"].as_str(),
        Some("started" | "interacted" | "interrupted")
    ) {
        return None;
    }
    let path = native["agentPath"].as_str()?;
    let description = path.strip_prefix("/root/").unwrap_or_else(|| {
        if path == "/root" {
            ""
        } else {
            path.rsplit(['/', '\\']).next().unwrap_or_default()
        }
    });
    let name = description
        .split('/')
        .filter_map(|part| {
            let label = part.replace(['_', '-'], " ");
            let mut letters = label.trim().chars();
            letters
                .next()
                .map(|first| format!("{}{}", first.to_uppercase(), letters.as_str()))
        })
        .collect::<Vec<_>>()
        .join(" / ");
    Some(
        json!({"type":"sub_agent", "childSessionId":child, "description":description,
        "subAgentType":if name.is_empty() { "Sub-agent" } else { &name }, "log":"", "actions":[]}),
    )
}

#[cfg(test)]
mod tests;
