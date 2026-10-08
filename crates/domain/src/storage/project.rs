//! Project configuration revisions, write outcomes, icon bytes and storage failures.

use serde_json::Value;

/// Filesystem revision used for optimistic `ait.json` writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectConfigRevision {
    /// Last modification time in Unix milliseconds.
    pub mtime_ms: f64,
    /// File size in bytes.
    pub size: f64,
}

/// Existing project configuration and the revision read with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectConfigDocument {
    /// Parsed JSON document, or none when `ait.json` is absent.
    pub config: Option<Value>,
    /// File revision, or none when `ait.json` is absent.
    pub revision: Option<ProjectConfigRevision>,
}

/// Result of an optimistic project configuration write.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectConfigWrite {
    /// The new document was installed atomically.
    Written {
        /// Installed document.
        config: Value,
        /// Revision after installation.
        revision: ProjectConfigRevision,
    },
    /// The on-disk revision did not match the caller's expectation.
    Stale {
        /// Current revision, or none when the file is absent.
        current_revision: Option<ProjectConfigRevision>,
    },
}

/// Safe project configuration storage failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectConfigStoreError {
    /// The existing file is unreadable or invalid JSON.
    #[error("invalid project config")]
    Invalid,
    /// The file could not be written or atomically installed.
    #[error("project config write failed")]
    Write,
}

/// Validated project icon bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIcon {
    /// Raw image bytes.
    pub bytes: Vec<u8>,
    /// MIME type detected from the bytes.
    pub mime_type: String,
}

/// Safe icon persistence and discovery failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectIconStoreError {
    /// Uploaded bytes are empty, too large, unsupported, non-square, or oversized in dimensions.
    #[error("invalid project icon")]
    Invalid,
    /// Icon persistence failed.
    #[error("project icon storage failed")]
    Io,
}
