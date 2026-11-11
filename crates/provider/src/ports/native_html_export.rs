//! Provider-native HTML export through the live native session, never a second writer.
//!
//! The adapter renders with the provider's own formatter and hands back an owned bounded
//! artifact. Native bytes are preserved verbatim: they may contain active scripts and
//! upstream viewer resources, so clients download them instead of rendering them inline.

use serde::Deserialize;

#[derive(Debug, serde::Serialize, Deserialize)]
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

/// Completed native export handed to the API for registration in the existing grant store.
#[derive(Debug)]
pub struct ExportedNativeHtml {
    /// Owned bounded file, removed when its grant or transfer is dropped.
    pub artifact: model::download::GeneratedArtifact,
}

/// Failure classes the adapter reports without exposing native paths or payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeHtmlExportError {
    /// The native renderer rejected the request or returned inconsistent facts.
    Rejected,
    /// The rendered file exceeded the shared artifact bound; nothing was truncated.
    TooLarge,
    /// Staging, adoption or transport I/O failed.
    Io,
}

impl From<NativeHtmlExportError> for crate::rpc::ErrorCode {
    fn from(error: NativeHtmlExportError) -> Self {
        match error {
            NativeHtmlExportError::Rejected => Self::InvalidMessage,
            NativeHtmlExportError::TooLarge => Self::ResourceExhausted,
            NativeHtmlExportError::Io => Self::AgentIo,
        }
    }
}
