//! Oh My Pi provider: one unmodified `omp --mode rpc-ui` process per session, driven over
//! OMP's native JSONL RPC, with native events mapped directly onto Ait session events.
//!
//! OMP owns credentials, extensions, tools and session files. The daemon registers [`OmpClient`].

mod commands;
mod config;
mod diagnostic;
mod events;
mod framing;
mod history;
mod interactions;
mod prompt;
mod roles;
mod sandbox;
mod session;
mod stream;
mod subagents;
mod tool_detail;
mod transport;
mod usage;

use std::ffi::OsString;
use std::path::PathBuf;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use sha2::Digest;

use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::environment::AgentEnvironment;
use provider_core::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use provider_core::protocol::provider::Details;

/// Stable provider identity recorded on handles, descriptors and host requests.
const PROVIDER: &str = "omp";
/// Product name shown in descriptors and diagnostics.
const LABEL: &str = "Oh My Pi";

/// Installed OMP executable with bounded RPC startup and discovery.
#[derive(Debug, Clone)]
pub struct OmpClient {
    program: PathBuf,
    timeouts: transport::Timeouts,
    environment: AgentEnvironment,
    images: provider_native::images::ImageStore,
    /// Role catalogs by cwd and environment identity; the client is already host-scoped.
    role_catalogs:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, RoleSnapshot>>>,
    /// Serializes native global preset read-modify-write operations across directory scopes.
    preset_writes: std::sync::Arc<tokio::sync::Mutex<()>>,
}

/// One cached role catalog plus the per-key read serialization gate.
#[derive(Debug, Clone)]
struct RoleSnapshot {
    catalog: Option<std::sync::Arc<roles::Catalog>>,
    identity: std::sync::Arc<()>,
    fresh: bool,
    gate: std::sync::Arc<tokio::sync::Mutex<()>>,
    updated: std::time::Instant,
}

const ROLE_CACHE_MAX: usize = 32;

impl OmpClient {
    /// Use `program` directly, inheriting the local OMP configuration and authentication.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            timeouts: transport::Timeouts::default(),
            environment: AgentEnvironment::default(),
            images: provider_native::images::ImageStore::default(),
            role_catalogs: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::BTreeMap::new(),
            )),
            preset_writes: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Store rendered native image output in the daemon's private `directory`.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = provider_native::images::ImageStore::new(directory);
        self
    }
}

impl OmpClient {
    /// Identity key for the role catalog: cwd plus the client environment (host-scoped client).
    fn role_catalog_key(&self, cwd: &str) -> String {
        let mut digest = sha2::Sha256::new();
        digest.update(cwd.as_bytes());
        digest.update([0]);
        for (key, value) in self.environment.entries() {
            digest.update(key.as_bytes());
            digest.update([0]);
            digest.update(value.as_bytes());
            digest.update([0]);
        }
        format!("{:x}", digest.finalize())
    }

    /// Read the role catalog for `cwd`, serialized per identity and cached (LRU 32).
    /// A stale-identity reply is never committed: the cache is keyed by the current identity.
    async fn role_catalog(
        &self,
        cwd: &str,
    ) -> Result<std::sync::Arc<roles::Catalog>, AgentSessionError> {
        let key = self.role_catalog_key(cwd);
        let gate = {
            let mut catalogs = self
                .role_catalogs
                .lock()
                .map_err(|_| AgentSessionError::Failed)?;
            if !catalogs.contains_key(&key) {
                if catalogs.len() >= ROLE_CACHE_MAX {
                    let oldest = catalogs
                        .iter()
                        .filter(|(_, snapshot)| std::sync::Arc::strong_count(&snapshot.gate) == 1)
                        .min_by_key(|(_, snapshot)| snapshot.updated)
                        .map(|(key, _)| key.clone())
                        .ok_or(AgentSessionError::Busy)?;
                    catalogs.remove(&oldest);
                }
                catalogs.insert(
                    key.clone(),
                    RoleSnapshot {
                        catalog: None,
                        identity: std::sync::Arc::new(()),
                        fresh: false,
                        gate: std::sync::Arc::new(tokio::sync::Mutex::new(())),
                        updated: std::time::Instant::now(),
                    },
                );
            }
            catalogs[&key].gate.clone()
        };
        let _guard = gate.lock().await;
        let (identity, previous) = {
            let mut catalogs = self
                .role_catalogs
                .lock()
                .map_err(|_| AgentSessionError::Failed)?;
            let snapshot = catalogs.get_mut(&key).ok_or(AgentSessionError::Failed)?;
            snapshot.updated = std::time::Instant::now();
            if snapshot.fresh
                && let Some(catalog) = &snapshot.catalog
            {
                return Ok(catalog.clone());
            }
            (snapshot.identity.clone(), snapshot.catalog.clone())
        };
        let result = roles::Catalog::read(self, cwd)
            .await
            .map(std::sync::Arc::new);
        let mut catalogs = self
            .role_catalogs
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        let snapshot = catalogs.get_mut(&key).ok_or(AgentSessionError::Failed)?;
        if !std::sync::Arc::ptr_eq(&identity, &snapshot.identity) {
            return Err(AgentSessionError::Failed);
        }
        match result {
            Ok(catalog) => {
                snapshot.catalog = Some(catalog.clone());
                snapshot.fresh = true;
                Ok(catalog)
            }
            Err(error) => previous.ok_or(error),
        }
    }

    /// Invalidate the cached role catalog for `cwd` (snapshot refresh entry point).
    fn invalidate_role_catalog(&self, cwd: &str) {
        let key = self.role_catalog_key(cwd);
        if let Ok(mut catalogs) = self.role_catalogs.lock()
            && let Some(snapshot) = catalogs.get_mut(&key)
        {
            snapshot.identity = std::sync::Arc::new(());
            snapshot.fresh = false;
        }
    }

    /// Global preset mutations can introduce or remove roles in every directory's catalog.
    fn invalidate_all_role_catalogs(&self) {
        if let Ok(mut catalogs) = self.role_catalogs.lock() {
            for snapshot in catalogs.values_mut() {
                snapshot.identity = std::sync::Arc::new(());
                snapshot.fresh = false;
            }
        }
    }
}

/// Resolve the installed OMP executable through `environment`.
///
/// Returns `AIT_SERVER_OMP_BIN` when set, without checking that it exists, otherwise `omp`
/// for PATH lookup. Hosts pass `std::env::var_os`; tests pass an isolated lookup.
#[must_use]
pub fn installed_program(environment: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    environment("AIT_SERVER_OMP_BIN").map_or_else(|| PathBuf::from("omp"), PathBuf::from)
}

#[async_trait::async_trait]
impl AgentClient for OmpClient {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn supports_write_confinement(&self) -> bool {
        true
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    async fn preset_descriptor(&self, cwd: &str) -> provider_core::ports::preset::PresetDescriptor {
        roles::presets::describe(self, cwd).await
    }

    async fn native_presets(
        &self,
        cwd: &str,
    ) -> Result<
        Vec<provider_core::ports::preset::PresetEntry>,
        provider_core::ports::preset::PresetError,
    > {
        roles::presets::list(self, cwd).await
    }

    async fn save_native_preset(
        &self,
        cwd: &str,
        preset: &provider_core::ports::preset::Preset,
    ) -> Result<(), provider_core::ports::preset::PresetError> {
        roles::presets::save(self, cwd, preset).await
    }

    async fn delete_native_preset(
        &self,
        cwd: &str,
        name: &str,
    ) -> Result<(), provider_core::ports::preset::PresetError> {
        roles::presets::delete(self, cwd, name).await
    }

    fn handles_out_of_band(&self, text: &str) -> bool {
        // Paseo's built-ins run beside foreground input; other slash text is a prompt (M5).
        commands::builtin(text).is_some()
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    /// Reject role assignments the live catalog cannot resolve before any session starts.
    ///
    /// Structural shape is already checked by [`config::validate`]. This gate runs before the
    /// process is created, so an unresolvable assignment never reaches a `--config` overlay.
    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::validate_spec(spec)?;
        if roles::overrides(&spec.config).is_empty() {
            return Ok(());
        }
        match self.role_catalog(&spec.cwd).await {
            Ok(catalog) => catalog.validate(&spec.config),
            // No catalog means no authority to accept an assignment; inherit is already
            // excluded above because it writes no overlay entry.
            Err(error) => {
                tracing::warn!(error=?error,"OMP role catalog unavailable; assignment rejected");
                Err(AgentSessionError::Rejected)
            }
        }
    }

    fn settings(&self, config: &StoredAgentConfig) -> Value {
        // Fast is the only feature, and only a live session's native state can offer it. A mode
        // change relaunches OMP between turns (D23); MCP servers and rewind are not offered yet.
        json!({"availableModes":config::modes(),"features":roles::stored_features(config),"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":true,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    fn refresh_discovery_cache(&self, cwd: &str) {
        self.invalidate_role_catalog(cwd);
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(provider_native::configuration::executable(&self.program))
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let mut probe = session::probe(self, cwd, self.timeouts).await?;
        let result = config::discover(&mut probe).await;
        let closed = probe.close().await;
        let mut details = result?;
        closed?;
        // Role discovery is read-only; on failure keep baseline features (stored keys only).
        match self.role_catalog(cwd).await {
            Ok(catalog) => catalog.append(&mut details, &StoredAgentConfig::default()),
            Err(error) => {
                tracing::warn!(error=?error,"OMP role catalog unavailable; baseline features kept");
                details
                    .features
                    .extend(roles::stored_features(&StoredAgentConfig::default()));
            }
        }
        Ok(details)
    }

    async fn draft_features(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        if !self.is_available().await? {
            return Err(AgentSessionError::Unavailable);
        }
        // As in Paseo, drafts offer no tier controls: Fast is read from a live session's state.
        // Role assignments apply through the overlay, so a draft can still offer them from the
        // discovered catalog.
        let mut features = Vec::new();
        match self.role_catalog(&spec.cwd).await {
            Ok(catalog) => features.extend(catalog.features(&spec.config)),
            Err(error) => {
                tracing::warn!(error=?error,"OMP role catalog unavailable; stored role keys kept");
                features.extend(roles::stored_features(&spec.config));
            }
        }
        Ok(features)
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        let timeouts = config::timeouts(&spec.config, self.timeouts);
        let mut probe = session::probe(self, &spec.cwd, timeouts).await?;
        let result = probe.request("get_available_commands", json!({})).await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        commands::list(&result)
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok(
                "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN.".to_owned(),
            );
        }
        // As in Paseo, no session process: `omp --version` and OMP's roots only (D31).
        Ok(diagnostic::report(self).await)
    }

    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<provider_core::protocol::timeline::NativeItem>, AgentSessionError> {
        Ok(history::inspect(self, handle, cwd).await?.entries)
    }

    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        history::inspect(self, handle, cwd).await
    }

    async fn list_sessions(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        history::list(self, options).await
    }

    async fn subagents(
        &self,
        cwd: &str,
    ) -> Result<Vec<provider_core::ports::controls::NativeSubagent>, AgentSessionError> {
        history::subagents(self, cwd).await
    }

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
    }

    async fn create_session_with_environment(
        &self,
        spec: &AgentSessionSpec,
        environment: &AgentEnvironment,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        let mut client = self.clone();
        let mut values: std::collections::BTreeMap<String, String> = client
            .environment
            .entries()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        values.extend(
            environment
                .entries()
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );

        client.environment = AgentEnvironment::try_from(values)?;
        client.create_session(spec).await
    }

    async fn resume_session(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        if purpose == AgentResumePurpose::History {
            return Err(AgentSessionError::Unavailable);
        }
        Ok(Box::new(session::open(self, spec, Some(handle)).await?) as Box<dyn AgentSession>)
    }
}

#[cfg(all(test, unix))]
mod tests;
