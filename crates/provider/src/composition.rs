//! Provider-owned assembly of built-in adapters and auxiliary generation.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use model::summary::{SummaryConfiguration, SummaryGenerator};

use crate::local::antigravity::AntigravityClient;
use crate::local::claude::ClaudeClient;
use crate::local::codex::CodexClient;
use crate::local::deepseek_harness::DeepSeekHarnessClient;
use crate::local::opencode::OpenCodeClient;
use crate::ports::agent_session::AgentClient;
use crate::service::agent_manager::{AgentManager, AgentManagerError};
use crate::service::summary_generation::Generation;

/// Configured built-in providers, with adapter identities and launch details kept in this crate.
#[derive(Debug)]
pub struct Providers {
    clients: Vec<Box<dyn AgentClient>>,
    summary_clients: Vec<Arc<dyn AgentClient>>,
}

impl Providers {
    /// Configure adapters from the process environment and installed native programs.
    ///
    /// `data_dir` is the daemon's persistent storage root, used for native image artifacts.
    /// Returns configured adapters without starting provider processes or creating storage.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self::configured(data_dir, |name| std::env::var_os(name))
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
        Arc::new(Generation::new(config, self.summary_clients.clone()))
    }

    /// Register the configured built-in adapters with the foreground `manager`.
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

    fn configured(data_dir: &Path, environment: impl Fn(&str) -> Option<OsString>) -> Self {
        let program = |name: &str, fallback: &str| {
            environment(name).map_or_else(|| PathBuf::from(fallback), PathBuf::from)
        };
        let images = data_dir.join("agents/provider-images");
        let codex = CodexClient::new(program("AIT_SERVER_CODEX_BIN", "codex"))
            .with_image_directory(images.clone());
        let claude = ClaudeClient::new(program("AIT_SERVER_CLAUDE_BIN", "claude"))
            .with_image_directory(images.clone());
        let antigravity = environment("AIT_SERVER_ANTIGRAVITY_BIN")
            .map_or_else(AntigravityClient::installed, |program| {
                AntigravityClient::new(program.into())
            });
        let opencode = OpenCodeClient::new(program("AIT_SERVER_OPENCODE_BIN", "opencode"));
        let mut dsh = environment("AIT_SERVER_DEEPSEEK_HARNESS_BIN")
            .map_or_else(DeepSeekHarnessClient::installed, |program| {
                DeepSeekHarnessClient::new(program.into())
            })
            .with_image_directory(images);
        if environment("AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT").as_deref() == Some("acp".as_ref()) {
            dsh = dsh.with_acp_profile();
        }
        let mut providers = Self {
            clients: Vec::new(),
            summary_clients: Vec::new(),
        };
        providers.add(codex);
        providers.add(claude);
        providers.add(antigravity);
        providers.add(opencode);
        providers.add(dsh);
        providers
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
