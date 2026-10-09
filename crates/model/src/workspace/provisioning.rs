//! Filesystem and Git observations used by project/workspace provisioning.

use std::fmt::Debug;

use domain::workspace::provisioning::{Checkout, DirectorySourceError};

/// Blocking adapter for local directory and lightweight Git inspection.
pub trait DirectorySource: Debug + Send + Sync {
    /// Resolve an existing directory and inspect its checkout placement.
    ///
    /// # Errors
    /// Returns a categorized filesystem error without modifying the directory.
    fn inspect(&self, path: &str) -> Result<Checkout, DirectorySourceError>;

    /// Create one empty child directory below an already normalized parent.
    ///
    /// # Errors
    /// Returns a categorized filesystem error without recursive creation.
    fn create_child(&self, parent: &str, name: &str) -> Result<String, DirectorySourceError>;

    /// Remove a directory only when it is empty.
    ///
    /// # Errors
    /// Returns a categorized filesystem error; non-empty directories are preserved.
    fn remove_empty(&self, path: &str) -> Result<(), DirectorySourceError>;

    /// Compare directory identities with realpath awareness where both paths exist.
    fn equivalent(&self, left: &str, right: &str) -> bool;

    /// Return the realpath spelling of an existing directory.
    ///
    /// # Errors
    /// Returns a categorized filesystem error for missing or unreadable paths.
    fn canonical(&self, path: &str) -> Result<String, DirectorySourceError>;
}
