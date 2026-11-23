//! Provider-native HTML export through the owning live session.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) agent_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CancelRequest {
    pub(crate) agent_id: String,
    pub(crate) token: String,
}

/// Native HTML held for registration in the authenticated download grant store.
#[derive(Debug)]
pub struct ExportedNativeHtml {
    /// Owned bounded file and its canonical Ait Agent identity.
    pub artifact: model::download::GeneratedArtifact,
}
