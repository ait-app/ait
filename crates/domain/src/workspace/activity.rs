//! Workspace activity shared by metadata services and their adapters.

use serde::{Deserialize, Serialize};

/// Paseo `WorkspaceStateBucket` values; legacy wire variants remain accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceStateBucket {
    /// Serialized as `needs_input`.
    #[serde(rename = "needs_input")]
    NeedsInput,
    /// Serialized as `failed`.
    #[serde(rename = "failed")]
    Failed,
    /// Serialized as `running`.
    #[serde(rename = "running")]
    Running,
    /// Serialized as `attention`.
    #[serde(rename = "attention")]
    Attention,
    /// Serialized as `done`.
    #[serde(rename = "done")]
    Done,
}

impl WorkspaceStateBucket {
    /// Return Paseo's priority: input, failure, running, attention, then done.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::NeedsInput => 0,
            Self::Failed => 1,
            Self::Running => 2,
            Self::Attention => 3,
            Self::Done => 4,
        }
    }
}
