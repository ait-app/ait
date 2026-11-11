//! Connection-owned compatibility views; shared runtime snapshots remain untouched.

use serde_json::Value;

/// Project known Agent response positions for a negotiated connection capability.
/// `value` must be a connection-owned reply, never the shared runtime snapshot.
pub(crate) fn response(value: &mut Value, secret_questions: bool) {
    if secret_questions {
        return;
    }
    if let Some(agent) = value.get_mut("agent") {
        snapshot(agent);
    }
    if let Some(final_snapshot) = value.get_mut("final") {
        snapshot(final_snapshot);
    }
    if let Some(agents) = value.get_mut("agents").and_then(Value::as_array_mut) {
        for agent in agents {
            snapshot(agent);
        }
    }
    if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries {
            if let Some(agent) = entry.get_mut("agent") {
                snapshot(agent);
            }
        }
    }
    // Directory preparation contains both the public page and all comparison rows.
    if let Some(prepared_response) = value.get_mut("response") {
        response(prepared_response, false);
    }
}

fn snapshot(agent: &mut Value) {
    let Some(requests) = agent
        .get_mut("pendingPermissions")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for request in requests {
        model::session::permission::project(request, false);
    }
}
