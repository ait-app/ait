//! Pure first-Agent input projection for Workspace naming.

use serde_json::Value;

/// Encode meaningful first-Agent source material without reading attachment files.
/// Empty or whitespace-only prompts with no attachments return none and retain naming eligibility.
#[must_use]
pub fn first_agent_source(prompt: Option<&str>, attachments: &[Value]) -> Option<String> {
    if prompt.is_none_or(|text| text.trim().is_empty()) && attachments.is_empty() {
        return None;
    }
    Some(serde_json::json!({"prompt":prompt,"attachments":attachments}).to_string())
}

#[cfg(test)]
mod tests;
