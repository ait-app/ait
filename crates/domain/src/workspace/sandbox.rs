//! OS-enforced write confinement for Agent processes in a sandboxed worktree Workspace.
//!
//! The Workspace record carries the isolation level; this module states which paths a
//! confined Agent process tree may write. Platform adapters resolve and enforce the policy.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::records::PersistedWorkspaceRecord;

/// Isolation layered on a Paseo-owned linked worktree, persisted with the Workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSandbox {
    /// Agent processes may write only the worktree, its Git metadata and provider state.
    WorktreeWrites,
}

/// Confinement requested for one Agent launch in a sandboxed Workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteConfinement {
    worktree_root: PathBuf,
}

impl WriteConfinement {
    /// Return the confinement required by `workspace`, or none for an unsandboxed Workspace.
    ///
    /// The recorded worktree root is preferred over the selected directory, which may be a
    /// subdirectory of the checkout.
    #[must_use]
    pub fn for_workspace(workspace: &PersistedWorkspaceRecord) -> Option<Self> {
        workspace.sandbox.map(|level| match level {
            WorkspaceSandbox::WorktreeWrites => Self {
                worktree_root: PathBuf::from(
                    workspace.worktree_root.as_deref().unwrap_or(&workspace.cwd),
                ),
            },
        })
    }

    /// Recorded worktree root, before platform path resolution.
    #[must_use]
    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }
}

/// Writable locations for one confined process tree; reads are never restricted.
///
/// Paths are absolute and already resolved to their real locations. Protected paths stay
/// read-only even inside an allowed tree, because unconfined processes execute them later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WritePolicy {
    trees: Vec<PathBuf>,
    prefixes: Vec<PathBuf>,
    protected: Vec<PathBuf>,
}

/// Git directory entries that unconfined Git or the daemon would execute or obey.
const PROTECTED_GIT_ENTRIES: [&str; 2] = ["config", "hooks"];

impl WritePolicy {
    /// Allow a linked worktree together with the Git directories that `git commit` writes.
    ///
    /// # Arguments
    ///
    /// * `worktree` - Real path of the checkout directory.
    /// * `git_dir` - Real path of the worktree's private Git directory.
    /// * `common_dir` - Real path of the repository's shared Git directory.
    ///
    /// # Returns
    ///
    /// A policy that allows all three trees and protects the worktree's `.git` pointer, the
    /// shared configuration and hooks, and the worktree-private configuration.
    #[must_use]
    pub fn worktree(worktree: &Path, git_dir: &Path, common_dir: &Path) -> Self {
        let mut protected = Vec::with_capacity(PROTECTED_GIT_ENTRIES.len() + 2);
        protected.push(worktree.join(".git"));
        protected.extend(
            PROTECTED_GIT_ENTRIES
                .iter()
                .map(|entry| common_dir.join(entry)),
        );
        protected.push(git_dir.join("config.worktree"));
        let mut trees = vec![worktree.to_path_buf(), common_dir.to_path_buf()];
        if !git_dir.starts_with(common_dir) {
            trees.push(git_dir.to_path_buf());
        }
        Self {
            trees,
            prefixes: Vec::new(),
            protected,
        }
    }

    /// Allow writes anywhere below `path`, including creating `path` itself.
    pub fn allow_tree(&mut self, path: PathBuf) {
        self.trees.push(path);
    }

    /// Allow writes to every path whose spelling starts with `path`, such as atomic-rename
    /// siblings of a state file.
    pub fn allow_prefix(&mut self, path: PathBuf) {
        self.prefixes.push(path);
    }

    /// Keep `path` and everything below it read-only, even inside an allowed tree.
    pub fn protect(&mut self, path: PathBuf) {
        self.protected.push(path);
    }

    /// Writable directory trees.
    #[must_use]
    pub fn trees(&self) -> &[PathBuf] {
        &self.trees
    }

    /// Writable path-spelling prefixes.
    #[must_use]
    pub fn prefixes(&self) -> &[PathBuf] {
        &self.prefixes
    }

    /// Read-only exceptions inside the writable locations.
    #[must_use]
    pub fn protected(&self) -> &[PathBuf] {
        &self.protected
    }
}

#[cfg(test)]
mod tests;
