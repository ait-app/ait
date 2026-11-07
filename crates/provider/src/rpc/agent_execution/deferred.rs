//! Owned control acknowledgements; awaiting them never borrows the live native session.

use std::{future::Future, pin::Pin};

use serde_json::{Value, json};

use super::ErrorCode;

/// One final RPC response whose native transaction can continue asking for user input.
pub(crate) type Response = Pin<Box<dyn Future<Output = Result<Value, ErrorCode>> + Send>>;

/// Finish a creation only after its initial input has settled admission.
/// Returns persistence errors without inventing a completed creation.
pub(super) fn created(
    creations: &model::creation::Creations,
    progress: &domain::creation::protocol::Snapshot,
    id: &str,
    snapshot: Value,
) -> Result<Value, ErrorCode> {
    let snapshot = creation_snapshot(snapshot);
    let progress =
        creations.advance(progress, "completed", Some(json!({"agent":snapshot})), None)?;
    let mut response =
        json!({"status":"agent_created","agentId":id,"error":null,"creation":progress});
    response["agent"] = snapshot;
    Ok(response)
}

/// Exclude connection-scoped editor and authentication UI from durable creation receipts.
pub(super) fn creation_snapshot(mut snapshot: Value) -> Value {
    if let Some(snapshot) = snapshot.as_object_mut() {
        snapshot.remove("providerUi");
    }
    snapshot
}

#[cfg(test)]
mod tests;
