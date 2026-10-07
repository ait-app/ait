//! Consumer-owned lifetime of Git observations for an active workspace directory.

use std::fmt::Debug;

/// A directory subscription's interest in workspace Git state.
/// Dropping the observation releases all of its paths.
pub trait WorkspaceGitObservation: Debug + Send + Sync {
    /// Replace the currently observed, non-archived workspace directories.
    fn set_paths(&mut self, paths: &[String]);
}

/// Git observation factory implemented by the filesystem capability.
pub trait WorkspaceGitObserver: Debug + Send + Sync {
    /// Create an initially empty observation after the subscription response is admitted.
    fn observe(&self) -> Box<dyn WorkspaceGitObservation>;
}
