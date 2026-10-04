//! Read only the canonical predecessors of a text item; never finalize its live suffix.
use serde_json::Value;

use super::normalize_messages;
use crate::local::opencode::{
    failure,
    http::Version,
    types::{Fault, ProtocolError, Record},
};

/// Return this input's history before a native text item, or `None` while it is unsettled.
/// Rejects missing admission, malformed identities and unsupported native content.
pub(in crate::local::opencode) fn before_text(
    version: Version,
    session: &str,
    input: &str,
    text: &str,
    messages: &[Value],
) -> Result<Option<Vec<Record>>, ProtocolError> {
    let start = messages
        .iter()
        .position(|message| match version {
            Version::V1 => message["info"]["role"] == "user" && message["info"]["id"] == input,
            Version::V2 => message["type"] == "user" && message["metadata"]["aitInputId"] == input,
        })
        .ok_or_else(|| {
            failure(
                Fault::RunRecoveryFailed,
                "streamed reply has no admitted input",
            )
        })?;
    let messages = &messages[start..];
    let target = messages
        .iter()
        .enumerate()
        .find_map(|(index, message)| match version {
            Version::V1 if message["info"]["role"] == "assistant" => message["parts"]
                .as_array()?
                .iter()
                .position(|part| part["type"] == "text" && part["id"] == text)
                .map(|part| (index, part)),
            Version::V2
                if message["type"] == "assistant"
                    && message["id"] == text
                    && message["content"][0]["type"] == "text" =>
            {
                Some((index, 0))
            }
            _ => None,
        });
    let Some((index, part)) = target else {
        return Ok(None);
    };
    let mut prefix = messages[..index].to_vec();
    if part > 0 {
        let mut current = messages[index].clone();
        current["parts"]
            .as_array_mut()
            .expect("located native text part")
            .truncate(part);
        prefix.push(current);
    }
    match normalize_messages(version, session, &prefix, part > 0) {
        Ok(records) => Ok(Some(records)),
        // A preceding assistant/tool may not yet be durable. Reconcile at completion instead
        // of assigning irreversible cursors from an incomplete or out-of-order observation.
        Err(error) if error.code == Fault::RunRecoveryFailed => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
