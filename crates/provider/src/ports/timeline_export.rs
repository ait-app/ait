//! Persisted Timeline export, independent of native-provider export commands.

use serde::{Deserialize, Serialize};

/// The two locally rendered, inert export formats.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// UTF-8 Markdown transcript.
    Markdown,
    /// Standalone HTML with escaped content and no active resources.
    Html,
}

/// Explicit selection of saved durable items; neither scope includes runtime editor state.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// User/assistant messages and saved voice transcripts only.
    Conversation,
    /// Every saved Timeline item, including durable tool and host result cards.
    Timeline,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) agent_id: String,
    pub(crate) format: Format,
    pub(crate) scope: Scope,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CancelRequest {
    pub(crate) agent_id: String,
    pub(crate) token: String,
}

/// Completed export handed to the API for registration in the existing authenticated grant store.
#[derive(Debug)]
pub struct ExportedTimeline {
    /// Owned bounded file, removed when its grant or transfer is dropped.
    pub artifact: model::download::GeneratedArtifact,
    /// Rendered format.
    pub format: Format,
    /// Applied durable-content selection.
    pub scope: Scope,
    /// Number of projected display entries included in the file.
    pub entries: u32,
}
