//! Read persisted daemon and project preferences through the shared summary configuration port.

use std::path::Path;
use std::sync::Arc;

use model::storage::daemon_config::DaemonConfigStore;
use model::storage::project::ProjectConfigStore;
use model::summary::{SummaryConfiguration, SummaryError};
use serde_json::Value;

use super::project_config::LocalProjectConfigStore;

/// Live daemon preferences and repository wording styles backed by local configuration files.
/// Blocking configuration reads must run outside an async reactor.
#[derive(Debug)]
pub struct LocalSummaryConfiguration {
    daemon: Arc<dyn DaemonConfigStore>,
}

impl LocalSummaryConfiguration {
    /// Bind the shared `daemon` store to local project configuration reads.
    /// Returns an adapter that reads current preferences on every call without caching a snapshot.
    #[must_use]
    pub fn new(daemon: Arc<dyn DaemonConfigStore>) -> Self {
        Self { daemon }
    }
}

impl SummaryConfiguration for LocalSummaryConfiguration {
    fn current(&self) -> Result<Value, SummaryError> {
        self.daemon.get().map_err(|_| SummaryError::Unavailable)
    }

    fn project(&self, cwd: &str) -> Value {
        let root = Path::new(cwd)
            .ancestors()
            .find(|path| path.join(".git").exists())
            .unwrap_or_else(|| Path::new(cwd));
        LocalProjectConfigStore
            .read(&root.to_string_lossy())
            .ok()
            .and_then(|document| document.config)
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests;
