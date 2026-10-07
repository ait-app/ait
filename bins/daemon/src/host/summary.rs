//! Bind persisted daemon and project preferences to provider summary generation.

use std::path::Path;
use std::sync::Arc;

use metadata::ports::daemon::DaemonConfigStore;
use metadata::ports::provisioning::ProjectConfigStore;
use metadata::storage::project_config::LocalProjectConfigStore;
use provider::summary::{SummaryConfiguration, SummaryError};
use serde_json::Value;

#[derive(Debug)]
pub(super) struct Configuration(pub(super) Arc<dyn DaemonConfigStore>);

impl SummaryConfiguration for Configuration {
    fn current(&self) -> Result<Value, SummaryError> {
        self.0.get().map_err(|_| SummaryError::Unavailable)
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
