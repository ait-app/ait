//! Project configuration and icon persistence ports over domain values.

use std::fmt::Debug;

use domain::storage::project::{
    ProjectConfigDocument, ProjectConfigRevision, ProjectConfigStoreError, ProjectConfigWrite,
    ProjectIcon, ProjectIconStoreError,
};
use serde_json::Value;

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
