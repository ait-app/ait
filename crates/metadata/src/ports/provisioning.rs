//! Project configuration/icon ports and shared directory provisioning contracts.

use serde_json::Value;
use std::fmt::Debug;

pub use model::workspace::provisioning::{
    Checkout, DirectorySource, DirectorySourceError, LEGACY_PROJECT_CONFIG_FILE_NAME,
    PROJECT_CONFIG_FILE_NAME,
};

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

/// Blocking adapter for one project's `ait.json` file.
pub trait ProjectConfigStore: Debug + Send + Sync {
    /// Read and parse `ait.json`; absence is a successful empty state.
    ///
    /// # Errors
    /// Returns `Invalid` for malformed/unreadable existing content.
    fn read(&self, root: &str) -> Result<ProjectConfigDocument, ProjectConfigStoreError>;

    /// Atomically install JSON only when the expected revision still matches.
    ///
    /// # Errors
    /// Returns `Write` when staging, serialization, or installation fails.
    fn write(
        &self,
        root: &str,
        config: &Value,
        expected_revision: Option<ProjectConfigRevision>,
    ) -> Result<ProjectConfigWrite, ProjectConfigStoreError>;
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

/// Blocking adapter for custom icon persistence and automatic project icon discovery.
pub trait ProjectIconStore: Debug + Send + Sync {
    /// Validate and atomically replace one project's custom icon.
    ///
    /// # Errors
    /// Returns `Invalid` for unsafe image bytes and `Io` for persistence failures.
    fn write_custom(&self, project_id: &str, bytes: &[u8]) -> Result<(), ProjectIconStoreError>;

    /// Remove a project's custom icon if present.
    ///
    /// # Errors
    /// Returns `Io` when removal fails for a reason other than absence.
    fn remove_custom(&self, project_id: &str) -> Result<(), ProjectIconStoreError>;

    /// Read and validate stored custom bytes. Missing or corrupt bytes resolve to none.
    ///
    /// # Errors
    /// Returns `Io` only when storage cannot be inspected safely.
    fn read_custom(&self, project_id: &str) -> Result<Option<ProjectIcon>, ProjectIconStoreError>;

    /// Find a small square project icon using Paseo's automatic file-name priorities.
    ///
    /// # Errors
    /// Returns `Io` when the project root cannot be inspected safely.
    fn find_automatic(
        &self,
        project_root: &str,
    ) -> Result<Option<ProjectIcon>, ProjectIconStoreError>;
}
