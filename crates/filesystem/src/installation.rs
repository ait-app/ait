//! Complete filesystem service installed as one capability component.

use crate::files::service::files::Files;
use crate::forge::service::forge::Forge;
use crate::git::service::{checkout::Checkout, git_fetch::GitFetch};

/// Required functional parts of the filesystem service; locks remain independently scoped.
#[derive(Debug)]
pub struct Dependencies {
    /// Git checkout operations.
    pub checkout: Checkout,
    /// Forge operations.
    pub forge: Forge,
    /// Scoped file operations and download grants.
    pub files: Files,
    /// Background fetches for observed repositories.
    pub git_fetch: GitFetch,
}

/// One complete filesystem service, installed and advertised at crate granularity.
#[derive(Debug)]
pub struct Service {
    dependencies: Dependencies,
}

impl Service {
    /// Construct the service from all required `dependencies`, without starting background work.
    /// # Returns
    /// A complete installation retaining each part's independent state and synchronization.
    #[must_use]
    pub fn new(dependencies: Dependencies) -> Self {
        Self { dependencies }
    }

    /// Borrow functional parts for cross-component composition.
    /// # Returns
    /// The required dependencies; borrowing does not install or remove individual capabilities.
    #[must_use]
    pub fn dependencies(&self) -> &Dependencies {
        &self.dependencies
    }

    /// Transfer the complete service into the host's dispatch and lifecycle composition.
    /// # Returns
    /// All functional parts, preserving their original resources and ownership.
    #[must_use]
    pub fn into_dependencies(self) -> Dependencies {
        self.dependencies
    }
}
