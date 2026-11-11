//! Existing native session discovery, preview, import, refresh and context export.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};

use super::timeline::{Cursor, NativeItem};

/// Filters for recent sessions which have not been actively imported.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecentRequest {
    /// Optional exact working directory.
    pub(crate) cwd: Option<String>,
    /// Registered providers, or all when omitted.
    pub(crate) providers: Option<Vec<String>>,
    /// Inclusive RFC3339 activity boundary.
    pub(crate) since: Option<String>,
    /// Positive result limit, at most 200; defaults to 20.
    pub(crate) limit: Option<usize>,
    /// Case-insensitive substring of title, prompt preview, handle or directory.
    pub(crate) query: Option<String>,
}

/// One read-only page of a native session's history; never creates or restores a writer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    /// Registered provider identity.
    pub(crate) provider_id: String,
    /// Native session identity from read-only discovery.
    pub(crate) provider_handle_id: String,
    /// Exact native working directory of the listed session.
    pub(crate) cwd: String,
    /// Opaque continuation of the same fixed cut; omitted reads the newest page.
    pub(crate) cursor: Option<String>,
    /// Positive display-item limit, at most 100; defaults to 50.
    pub(crate) limit: Option<usize>,
}

/// Borrowed preview wire projection; absent native turn identities are omitted, not null.
/// This leaves the persisted `NativeItem` representation unchanged.
pub(crate) struct PreviewEntries<'a>(pub(crate) &'a [NativeItem]);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewEntry<'a> {
    key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    timestamp: &'a str,
    item: &'a serde_json::Value,
}

impl Serialize for PreviewEntries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|entry| PreviewEntry {
            key: &entry.key,
            turn_id: entry.turn_id.as_deref(),
            timestamp: &entry.timestamp,
            item: &entry.item,
        }))
    }
}

/// Import an existing provider handle. Legacy field aliases must agree when both are supplied.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImportRequest {
    /// Provider identity.
    pub(crate) provider_id: Option<String>,
    /// Legacy provider identity.
    pub(crate) provider: Option<String>,
    /// Native session identity.
    pub(crate) provider_handle_id: Option<String>,
    /// Legacy native identity.
    pub(crate) session_id: Option<String>,
    /// Existing absolute directory, checked against the native session.
    pub(crate) cwd: String,
    /// Explicit matching Workspace; omitted uses metadata's directory opening service.
    pub(crate) workspace_id: Option<String>,
    /// User-visible Agent labels.
    #[serde(default)]
    pub(crate) labels: BTreeMap<String, String>,
}

/// Select all context, or an inclusive timeline boundary.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForkRequest {
    /// Full Agent ID, unique prefix or title.
    pub(crate) agent_id: String,
    /// Preferred inclusive sequence boundary.
    pub(crate) boundary_cursor: Option<Cursor>,
    /// Alternative inclusive assistant-message boundary.
    pub(crate) boundary_message_id: Option<String>,
}

/// Compress older imported chat context using the destination Agent's exact model selection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextCompactRequest {
    /// Destination working directory for provider session setup.
    pub cwd: String,
    /// Selected destination provider.
    pub provider: String,
    /// Selected destination model, or its configured default.
    pub model: Option<String>,
    /// Selected destination reasoning option, when available.
    pub thinking_option_id: Option<String>,
    /// One bounded segment of imported conversation context.
    pub context: String,
}

/// Encode one preview continuation as an opaque wire token; clients echo it verbatim.
/// # Errors
/// Returns a serialization failure when the cursor cannot be represented.
pub(crate) fn encode_preview_cursor(
    cursor: &crate::ports::native_history::PreviewCursor,
) -> Result<String, serde_json::Error> {
    use base64::Engine;
    let bytes = serde_json::to_vec(cursor)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Decode an opaque preview continuation without trusting its fields; adapters revalidate
/// identity and cut binding before any native read.
/// # Errors
/// Returns a rejection for oversized, malformed or non-object tokens.
pub(crate) fn decode_preview_cursor(
    encoded: &str,
) -> Result<crate::ports::native_history::PreviewCursor, ()> {
    use base64::Engine;
    if encoded.len() > 4096 {
        return Err(());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

#[cfg(test)]
mod tests;
