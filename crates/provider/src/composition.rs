//! Adapter container and the auxiliary generation assembled from it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use model::summary::{SummaryConfiguration, SummaryGenerator};

use crate::local::codex::CodexClient;
use crate::local::opencode::OpenCodeClient;
use crate::local::pi::PiClient;
use crate::ports::usage_ledger::{NoopUsageLedger, UsageLedger};
use crate::service::agent_manager::{AgentManager, AgentManagerError};
use crate::service::summary_generation::Generation;
use crate::storage::usage_ledger::SqliteUsageLedger;
use provider_core::ports::agent_session::AgentClient;

/// Configured provider adapters, registered with the foreground manager and used for auxiliary
/// generation. `provider-builtin` decides which adapters it holds.
#[derive(Debug)]
pub struct Providers {
    pub(crate) clients: Vec<Box<dyn AgentClient>>,
    pub(crate) summary_clients: Vec<Arc<dyn AgentClient>>,
    /// Ledger that records the provider calls of auxiliary generation.
    usage_ledger: Arc<dyn UsageLedger>,
    images: PathBuf,
}

impl Providers {
    /// Record the provider calls of generators assembled afterwards in the host `ledger`.
    #[must_use]
    pub fn with_usage_ledger(mut self, ledger: Arc<SqliteUsageLedger>) -> Self {
        self.usage_ledger = ledger;
        self
    }

    /// Assemble isolated structured generation using adapters that implement that capability.
    ///
    /// `config` supplies live provider and model preferences. Returns a shared generator;
    /// auxiliary sessions do not enter the foreground Agent registry.
    #[must_use]
    pub fn summary_generator(
        &self,
        config: Arc<dyn SummaryConfiguration>,
    ) -> Arc<dyn SummaryGenerator> {
        Arc::new(
            Generation::new(config, self.summary_clients.clone())
                .with_usage_ledger(self.usage_ledger.clone()),
        )
    }

    /// Register the configured adapters with the foreground `manager`.
    ///
    /// Returns success after registering every adapter, without probing native programs.
    ///
    /// # Errors
    /// Returns the manager's identity validation or duplicate-registration error. Adapters
    /// registered before an error remain registered.
    pub fn register(self, manager: &mut AgentManager) -> Result<(), AgentManagerError> {
        for client in self.clients {
            manager.register_client(client)?;
        }
        Ok(())
    }

    /// Configure the built-in adapters that still live in this crate, reading launcher
    /// overrides from `environment`; `provider-builtin` appends the extracted adapters.
    ///
    /// `data_dir` is the daemon's persistent storage root and `environment` resolves variables
    /// such as `AIT_SERVER_CODEX_BIN`, letting in-process hosts select offline native programs
    /// without mutating the process environment. Returns adapters without starting processes.
    #[doc(hidden)]
    #[must_use]
    pub fn configured(data_dir: &Path, environment: impl Fn(&str) -> Option<OsString>) -> Self {
        let program = |name: &str, fallback: &str| {
            environment(name).map_or_else(|| PathBuf::from(fallback), PathBuf::from)
        };
        let images = data_dir.join("agents/provider-images");
        let codex = CodexClient::new(program("AIT_SERVER_CODEX_BIN", "codex"))
            .with_image_directory(images.clone());
        let opencode = OpenCodeClient::new(program("AIT_SERVER_OPENCODE_BIN", "opencode"));
        let pi =
            PiClient::new(program("AIT_SERVER_PI_BIN", "pi")).with_image_directory(images.clone());
        let mut providers = Self {
            clients: Vec::new(),
            summary_clients: Vec::new(),
            usage_ledger: Arc::new(NoopUsageLedger),
            images,
        };
        providers.add(codex);
        providers.add(opencode);
        providers.add(pi);
        providers
    }

    /// Add an adapter owned by another crate, built for the shared native image directory.
    ///
    /// `build` receives the directory where adapters store rendered native image artifacts.
    /// Returns the assembly with the adapter appended; nothing is started or created.
    #[must_use]
    pub fn with<C>(mut self, build: impl FnOnce(&Path) -> C) -> Self
    where
        C: AgentClient + Clone + 'static,
    {
        let client = build(&self.images);
        self.add(client);
        self
    }

    fn add(&mut self, client: impl AgentClient + Clone + 'static) {
        if client.supports_summary_generation() {
            self.summary_clients.push(Arc::new(client.clone()));
        }
        self.clients.push(Box::new(client));
    }
}

#[cfg(test)]
mod tests;
