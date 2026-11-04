//! Tool display summaries used in chat-history attachments, without raw tool input.

use std::borrow::Cow;

use serde_json::Value;

pub(super) fn summary(item: &Value) -> String {
    let name = item["name"].as_str().unwrap_or("Tool");
    let detail = &item["detail"];
    let (display, mut summary) = match detail["type"].as_str() {
        Some("shell") => (Some("Shell"), detail["command"].as_str()),
        Some("read") => (Some("Read"), detail["filePath"].as_str()),
        Some("edit") => (Some("Edit"), detail["filePath"].as_str()),
        Some("write") => (Some("Write"), detail["filePath"].as_str()),
        Some("search") => (Some("Search"), detail["query"].as_str()),
        Some("fetch") => (Some("Fetch"), detail["url"].as_str()),
        Some("worktree_setup") => (Some("Worktree setup"), detail["branchName"].as_str()),
        Some("sub_agent") => (
            Some(
                detail["subAgentType"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Task"),
            ),
            detail["description"].as_str(),
        ),
        Some("plain_text") => (None, detail["label"].as_str()),
        Some("plan") => (Some("Plan"), None),
        _ => (None, None),
    };
    let mut display = display.map_or_else(|| Cow::Owned(humanize(name)), Cow::Borrowed);
    if name.trim().eq_ignore_ascii_case("terminal") {
        display = Cow::Borrowed("Terminal");
    } else if detail["type"] == "unknown" && name.trim().eq_ignore_ascii_case("task") {
        display = Cow::Borrowed("Task");
        summary = item["metadata"]["subAgentActivity"].as_str();
    }
    let summary = summary
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if summary.is_empty() {
        return format!("[{display}]");
    }
    let summary = if summary.encode_utf16().count() > 200 {
        let units = summary.encode_utf16().take(197).collect::<Vec<_>>();
        format!("{}...", String::from_utf16_lossy(&units))
    } else {
        summary
    };
    format!("[{display}] {summary}")
}

/// MCP server names the daemon's own agent tools appear under: `ait` is the
/// injected server, `paseo` covers transcripts recorded before the rename.
const AIT_TOOL_NAMESPACES: [&str; 2] = ["ait", "paseo"];

fn is_ait_tool_namespace(segment: &str) -> bool {
    AIT_TOOL_NAMESPACES.iter().any(|namespace| {
        segment
            .strip_prefix(namespace)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
    })
}

fn humanize(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return name.to_owned();
    }
    let normalized = trimmed.to_lowercase();
    let leaf = normalized
        .rsplit(|character: char| !character.is_ascii_alphanumeric())
        .find(|token| !token.is_empty());
    if leaf != Some("speak") {
        if normalized.contains("__") {
            let segments: Vec<_> = normalized
                .split("__")
                .filter(|segment| !segment.is_empty())
                .collect();
            if segments.len() >= 3 && segments[0] == "mcp" && is_ait_tool_namespace(segments[1]) {
                return humanize(&segments[2..].join("__"));
            }
        } else if let Some((namespace, leaf)) = normalized.split_once('.')
            && is_ait_tool_namespace(namespace)
        {
            return humanize(leaf);
        }
    }
    if trimmed.contains([':', '.', '/']) || trimmed.contains("__") {
        return trimmed.to_owned();
    }
    let words = trimmed
        .split(['_', '-', ' '])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut characters = words.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

#[cfg(test)]
mod tests;
