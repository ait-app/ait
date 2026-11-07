//! Oh My Pi's native JSONL RPC adapter. OMP owns credentials, extensions, tools and session files.

mod actions;
mod bridge;
mod btw;
mod commands;
mod completion;
mod config;
mod framing;
mod history;
mod interactions;
pub(crate) mod native_host;
mod roles;
mod session;
mod stream;
mod subagents;
mod transport;
mod ui;
mod vibe;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::provider::Details;

#[derive(Debug, Clone)]
struct RoleSnapshot {
    catalog: Arc<roles::Catalog>,
    default_details: Option<Details>,
    updated: Instant,
}
/// Stable provider identity recorded on handles, descriptors and host requests.
const PROVIDER: &str = "omp";
/// Product name shown in descriptors and diagnostics.
const LABEL: &str = "Oh My Pi";

/// Installed OMP executable with bounded RPC startup and discovery.
#[derive(Debug, Clone)]
pub(crate) struct OmpClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
    completion_support: Arc<RwLock<Option<bool>>>,
    role_catalogs: Arc<RwLock<BTreeMap<String, RoleSnapshot>>>,
    metadata_loads: Arc<Mutex<BTreeMap<String, Weak<tokio::sync::Mutex<()>>>>>,
    native_host: Option<native_host::NativeHost>,
}

impl OmpClient {
    /// Use `program` directly, inheriting the local OMP configuration and authentication.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
            completion_support: Arc::default(),
            role_catalogs: Arc::default(),
            metadata_loads: Arc::default(),
            native_host: None,
        }
    }

    /// Resolve the private runtime installed under `data_dir` for `environment`.
    ///
    /// Returns `None` when nothing is installed or verification fails, which keeps the
    /// official binary and leaves the Vibe bridge capability unavailable.
    pub(crate) fn native_host_runtime(
        data_dir: &std::path::Path,
        environment: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<native_host::NativeHost> {
        native_host::discover(data_dir, environment)
    }

    /// Adopt a verified private native host runtime instead of the official binary.
    ///
    /// The runtime keeps the same data directory, credentials and session files as the
    /// official `omp`: only the process that speaks RPC changes. A runtime that fails
    /// verification is never adopted, so the caller keeps the official binary.
    #[must_use]
    pub(crate) fn with_native_host(mut self, host: Option<native_host::NativeHost>) -> Self {
        self.native_host = host;
        self
    }

    /// Verified native host runtime for this client, if one is installed.
    pub(crate) fn native_host(&self) -> Option<&native_host::NativeHost> {
        self.native_host.as_ref()
    }

    /// Store rendered native image output in the daemon's private `directory`.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }

    async fn service_tiers(
        &self,
        cwd: &str,
        model: Option<&str>,
    ) -> Result<config::ServiceTiers, AgentSessionError> {
        let mut probe = session::probe(self, cwd, model).await?;
        let result = config::service_tiers(&mut probe).await;
        let closed = probe.close().await;
        let tiers = result?;
        closed?;
        Ok(tiers)
    }

    async fn role_settings(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<(Arc<roles::Catalog>, (Option<String>, config::ServiceTiers)), AgentSessionError>
    {
        let _metadata = self.metadata_gate(&spec.cwd)?.lock_owned().await;
        let cached = self.cached_roles(&spec.cwd)?;
        let fresh = cached.is_none();
        let mut roles = match cached {
            Some(roles) => roles,
            None => Arc::new(roles::Catalog::read(self, &spec.cwd).await?),
        };
        let mut probe = session::probe_config(
            self,
            &spec.cwd,
            spec.config.model.as_deref(),
            Some(&spec.config),
        )
        .await.inspect_err(|error|tracing::warn!(phase="role_probe",error=?error,"OMP role probe could not start"))?;
        let result: Result<_, AgentSessionError> = async {
            if fresh {
                Arc::make_mut(&mut roles).read_chat(&mut probe).await?;
            }
            roles.validate(&spec.config)?;
            let state = probe.request("get_state", json!({})).await?;
            Ok((
                config::runtime(&state)?.model,
                config::service_tiers(&mut probe).await?,
            ))
        }
        .await;
        let closed = probe.close().await;
        let tiers = result?;
        closed?;
        if fresh {
            self.cache_roles(&spec.cwd, &roles, None)?;
        }
        Ok((roles, tiers))
    }

    fn cache_roles(
        &self,
        cwd: &str,
        roles: &Arc<roles::Catalog>,
        default_details: Option<&Details>,
    ) -> Result<(), AgentSessionError> {
        let key = self.role_catalog_key(cwd);
        let roles = roles.clone();
        let default_details = default_details.cloned();
        let mut catalogs = self
            .role_catalogs
            .write()
            .map_err(|_| AgentSessionError::Failed)?;
        if catalogs.len() >= 32
            && !catalogs.contains_key(&key)
            && let Some(oldest) = catalogs
                .iter()
                .min_by_key(|(_, snapshot)| snapshot.updated)
                .map(|(key, _)| key.clone())
        {
            catalogs.remove(&oldest);
        }
        catalogs.insert(
            key,
            RoleSnapshot {
                catalog: roles,
                default_details,
                updated: Instant::now(),
            },
        );
        Ok(())
    }

    fn metadata_gate(&self, cwd: &str) -> Result<Arc<tokio::sync::Mutex<()>>, AgentSessionError> {
        let key = self.role_catalog_key(cwd);
        let mut gates = self
            .metadata_loads
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
            return Ok(gate);
        }
        gates.retain(|_, gate| gate.strong_count() > 0);
        if gates.len() >= 32 {
            return Err(AgentSessionError::Busy);
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        gates.insert(key, Arc::downgrade(&gate));
        Ok(gate)
    }

    fn cached_roles(&self, cwd: &str) -> Result<Option<Arc<roles::Catalog>>, AgentSessionError> {
        let key = self.role_catalog_key(cwd);
        Ok(self
            .role_catalogs
            .read()
            .map_err(|_| AgentSessionError::Failed)?
            .get(&key)
            .map(|snapshot| snapshot.catalog.clone()))
    }

    fn role_catalog_key(&self, cwd: &str) -> String {
        let mut digest = Sha256::new();
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
}

/// Serve the OMP bridge on a caller-owned JSONL stream without altering native installation.
/// Returns bounded protocol, process startup, or I/O errors; EOF closes the native runtime.
pub(crate) async fn serve_acp<R, W>(program: PathBuf, input: R, output: W) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    bridge::serve(OmpClient::new(program), input, output).await
}

#[async_trait::async_trait]
impl AgentClient for OmpClient {
    fn handles_out_of_band(&self, text: &str) -> bool {
        // `/vibe` is declared here so an inactive owner can be woken for it; the live session
        // decides whether the native host bridge can actually serve it.
        commands::is_btw(text) || commands::is_vibe(text) || commands::is_query(text)
    }
    fn discovery_cache_ttl(&self) -> Duration {
        Duration::MAX
    }

    fn discovery_timeout(&self) -> Duration {
        // Config, model catalog and RPC each cold-start an official native process.
        // The catalog owner runs this in the background, outside all Agent lanes.
        transport::STARTUP_TIMEOUT.saturating_mul(3)
    }

    async fn complete_commands(
        &self,
        spec: &AgentSessionSpec,
        input: &crate::protocol::commands::CompletionInput,
    ) -> Result<crate::protocol::commands::ValidatedCompletion, AgentSessionError> {
        config::validate_spec(spec)?;
        input.validate()?;
        if *self
            .completion_support
            .read()
            .map_err(|_| AgentSessionError::Failed)?
            == Some(false)
        {
            return Ok(crate::protocol::commands::ValidatedCompletion::default());
        }
        let mut probe = session::probe(self, &spec.cwd, spec.config.model.as_deref()).await?;
        let result = async {
            let native = if probe.command_completion() {
                config::commands(&probe.request("get_available_commands", json!({})).await?)?
            } else {
                Vec::new()
            };
            // A metadata probe never owns adapter Vibe state, so `/vibe` stays filtered here.
            completion::complete(&mut probe, input, &native, false).await
        }
        .await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        Ok(result)
    }
    fn supports_session_commands(&self) -> bool {
        true
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn supports_write_confinement(&self) -> bool {
        true
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::validate_spec(spec)?;
        if spec
            .config
            .feature_values
            .iter()
            .flatten()
            .any(|(key, value)| roles::is_key(key) && value != "native")
        {
            let key = self.role_catalog_key(&spec.cwd);
            let cached = self
                .role_catalogs
                .read()
                .map_err(|_| AgentSessionError::Failed)?
                .get(&key)
                .map(|snapshot| snapshot.catalog.validate(&spec.config));
            if let Some(result) = cached {
                return result;
            }
            self.role_settings(spec).await?;
        }
        Ok(())
    }

    async fn resolve_selection(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<StoredAgentConfig, AgentSessionError> {
        config::validate_spec(spec)?;
        if !spec
            .config
            .feature_values
            .as_ref()
            .is_some_and(|values| values.get("modelPreset").is_some_and(Value::is_string))
        {
            return Ok(spec.config.clone());
        }
        if self.cached_roles(&spec.cwd)?.is_none() {
            self.discover(&spec.cwd).await?;
        }
        self.cached_roles(&spec.cwd)?
            .ok_or(AgentSessionError::Failed)?
            .resolve_preset(&spec.config)
    }

    fn settings(&self, config: &StoredAgentConfig) -> Value {
        let mut features = config::features(config, config::ServiceTiers::default());
        features.extend(roles::stored_features(config));
        let mut ui = ui::State::default().snapshot();
        ui["actions"] = actions::catalog();
        json!({"providerUi":ui,"availableModes":config::modes(),"features":features,"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(super::configuration::executable(&self.program))
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let started = Instant::now();
        let _metadata = self.metadata_gate(cwd)?.lock_owned().await;
        let key = self.role_catalog_key(cwd);
        let completed = self
            .role_catalogs
            .read()
            .map_err(|_| AgentSessionError::Failed)?
            .get(&key)
            .filter(|snapshot| snapshot.updated >= started)
            .cloned();
        if let Some(details) = completed
            .as_ref()
            .and_then(|snapshot| snapshot.default_details.clone())
        {
            return Ok(details);
        }
        let mut roles = match completed {
            Some(snapshot) => snapshot.catalog,
            None => Arc::new(roles::Catalog::read(self, cwd).await?),
        };
        let mut probe = session::probe(self, cwd, None).await?;
        Arc::make_mut(&mut roles).read_chat(&mut probe).await?;
        let result = config::discover(&mut probe).await;
        let closed = probe.close().await;
        let mut details = result?;
        closed?;
        roles.append(&mut details, &StoredAgentConfig::default());
        self.cache_roles(cwd, &roles, Some(&details))?;
        Ok(details)
    }

    async fn draft_features(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        // Role menus request the default profile after background discovery completes.
        // Reuse that completed snapshot instead of cold-starting three more native readers.
        if spec.config == StoredAgentConfig::default() {
            let key = self.role_catalog_key(&spec.cwd);
            let cached = self
                .role_catalogs
                .read()
                .map_err(|_| AgentSessionError::Failed)?
                .get(&key)
                .and_then(|snapshot| {
                    snapshot
                        .default_details
                        .as_ref()
                        .map(|details| details.features.clone())
                });
            if let Some(features) = cached {
                return Ok(features);
            }
        }
        let (roles, tiers) = self.role_settings(spec).await?;
        let mut features = config::features(&spec.config, tiers.1);
        features.extend(roles.features(&spec.config));
        Ok(features)
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        let mut probe = session::probe(self, &spec.cwd, None).await?;
        let result = probe.request("get_available_commands", json!({})).await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        config::commands(&result).map(|native| commands::visible(&native))
    }

    /// OMP owns its presets, so it answers for the native store instead of the Ait one.
    fn owns_native_presets(&self) -> bool {
        true
    }

    /// OMP owns its presets, so the unified editor is answered by the native store here.
    async fn preset_descriptor(&self, cwd: &str) -> crate::ports::preset::PresetDescriptor {
        roles::presets::editor::describe(self, cwd).await
    }

    async fn native_presets(
        &self,
        cwd: &str,
    ) -> Result<Vec<crate::ports::preset::Preset>, crate::ports::preset::PresetError> {
        roles::presets::editor::list(self, cwd).await
    }

    async fn save_native_preset(
        &self,
        cwd: &str,
        preset: &crate::ports::preset::Preset,
    ) -> Result<(), crate::ports::preset::PresetError> {
        roles::presets::editor::save(self, cwd, preset).await
    }

    async fn delete_native_preset(
        &self,
        cwd: &str,
        name: &str,
    ) -> Result<(), crate::ports::preset::PresetError> {
        roles::presets::editor::delete(self, cwd, name).await
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok(
                "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN.".to_owned(),
            );
        }
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
        let details = self
            .discover(cwd.to_str().ok_or(AgentSessionError::Failed)?)
            .await?;
        Ok(format!(
            "Oh My Pi RPC ready; {} available models",
            details.models.len()
        ))
    }

    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<crate::protocol::timeline::NativeItem>, AgentSessionError> {
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

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(bridge::Session::open(self, spec, None).await?) as Box<dyn AgentSession>)
    }

    async fn create_session_with_environment(
        &self,
        spec: &AgentSessionSpec,
        environment: &AgentEnvironment,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        let mut client = self.clone();
        let mut values: BTreeMap<String, String> = client
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
        Ok(
            Box::new(bridge::Session::open(self, spec, Some(handle)).await?)
                as Box<dyn AgentSession>,
        )
    }
}

#[cfg(all(test, unix))]
mod tests;
