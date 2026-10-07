//! Shared naming input and the Git branch rename boundary.

use serde_json::Value;
use std::fmt::Debug;

/// Blocking Git boundary for renaming a still-eligible managed placeholder branch.
pub trait WorkspaceBranchNamer: Debug + Send + Sync {
    /// Rename only when `cwd` remains managed and its current branch equals `expected`.
    /// Returns the chosen collision-free branch, or none when no safe rename is possible.
    fn rename(&self, cwd: &str, expected: &str, desired: &str) -> Option<String>;
}

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
