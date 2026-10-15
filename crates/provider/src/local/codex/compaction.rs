//! Discover native background compaction without guessing support from a CLI version.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::Transport;
use crate::ports::agent_session::AgentSessionError;

/// Return native enablement when the flag exists, or `None` for unsupported/retired flags.
///
/// # Errors
/// Returns transport failures, malformed feature pages, and non-terminating pagination.
pub(super) async fn inspect(transport: &mut Transport) -> Result<Option<bool>, AgentSessionError> {
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    loop {
        let response = match transport
            .request(
                "experimentalFeature/list",
                json!({"cursor":cursor,"limit":100}),
            )
            .await
        {
            Ok(response) => response,
            Err(AgentSessionError::Rejected) => return Ok(None),
            Err(error) => return Err(error),
        };
        let features = response["data"]
            .as_array()
            .filter(|features| features.len() <= 100)
            .ok_or(AgentSessionError::Failed)?;
        if let Some(feature) = features
            .iter()
            .find(|feature| feature["name"] == "async_compaction")
        {
            return match feature["stage"].as_str() {
                Some("beta" | "underDevelopment" | "stable") => feature["enabled"]
                    .as_bool()
                    .map(Some)
                    .ok_or(AgentSessionError::Failed),
                _ => Ok(None),
            };
        }
        cursor = match &response["nextCursor"] {
            Value::Null => return Ok(None),
            Value::String(next) if !next.is_empty() => Some(next.clone()),
            _ => return Err(AgentSessionError::Failed),
        };
        if seen.len() >= 64 || !seen.insert(cursor.clone()) {
            return Err(AgentSessionError::Failed);
        }
    }
}
