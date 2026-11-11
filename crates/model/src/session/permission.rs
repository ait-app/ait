//! Portable `PermissionRequest` compatibility projection for connection-owned payloads.

use serde_json::{Value, json};

const UPGRADE_MESSAGE: &str =
    "Upgrade this client to answer secret questions. You can cancel this request here.";

/// Replace a secret `PermissionRequest` with a cancel-only tool card for an older client.
///
/// `request` is an owned outbound `PermissionRequest`, not shared runtime state.
/// `secret_questions` is the negotiated compatibility capability, not authorization.
/// Capable clients and non-secret requests are left unchanged without cloning.
/// Returns nothing; routing identity is preserved and answer-bearing fields are removed.
pub fn project(request: &mut Value, secret_questions: bool) {
    if secret_questions {
        return;
    }
    let secret = request["input"]["questions"]
        .as_array()
        .is_some_and(|questions| questions.iter().any(|question| question["secret"] == true));
    if !secret {
        return;
    }
    // Rebuild an allowlisted card: labels, detail, metadata and suggestions can contain input.
    let id = request["id"].take();
    let provider = request["provider"].take();
    *request = json!({
        "id": id,
        "provider": provider,
        "name": "secret_question",
        "kind": "tool",
        "title": "Client upgrade required",
        "description": UPGRADE_MESSAGE,
        "actions": [{"id": "deny", "label": "Cancel", "behavior": "deny"}]
    });
}

#[cfg(test)]
mod tests;
