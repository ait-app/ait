//! Oh My Pi's native JSONL RPC adapter. OMP owns credentials, extensions, tools and session files.

mod aliases;
mod bridge;
mod config;
mod framing;
mod history;
mod interactions;
mod roles;
mod session;
mod stream;
mod subagents;
mod transport;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::provider::Details;

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
    role_catalogs: Arc<Mutex<BTreeMap<String, RoleSnapshot>>>,
}

/// One cached role catalog plus the per-key read serialization gate.
#[derive(Debug, Clone)]
struct RoleSnapshot {
    catalog: Option<Arc<roles::Catalog>>,
    identity: Arc<()>,
    fresh: bool,
    gate: Arc<tokio::sync::Mutex<()>>,
    updated: std::time::Instant,
}

const ROLE_CACHE_MAX: usize = 32;

impl OmpClient {
    /// Use `program` directly, inheriting the local OMP configuration and authentication.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
            role_catalogs: Arc::new(Mutex::new(BTreeMap::new())),
        }
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
        confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
    ) -> Result<config::ServiceTiers, AgentSessionError> {
        let mut probe = session::probe(self, cwd, model, confinement).await?;
        let result = config::service_tiers(&mut probe).await;
        let closed = probe.close().await;
        let tiers = result?;
        closed?;
        Ok(tiers)
    }

    /// Identity key for the role catalog: cwd plus the client environment (host-scoped client).
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

    /// Read the role catalog for `cwd`, serialized per identity and cached (LRU 32).
    /// A stale-identity reply is never committed: the cache is keyed by the current identity.
    async fn role_catalog(&self, cwd: &str) -> Result<Arc<roles::Catalog>, AgentSessionError> {
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
                        .filter(|(_, snapshot)| Arc::strong_count(&snapshot.gate) == 1)
                        .min_by_key(|(_, snapshot)| snapshot.updated)
                        .map(|(key, _)| key.clone())
                        .ok_or(AgentSessionError::Busy)?;
                    catalogs.remove(&oldest);
                }
                catalogs.insert(
                    key.clone(),
                    RoleSnapshot {
                        catalog: None,
                        identity: Arc::new(()),
                        fresh: false,
                        gate: Arc::new(tokio::sync::Mutex::new(())),
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
        let result = roles::Catalog::read(self, cwd).await.map(Arc::new);
        let mut catalogs = self
            .role_catalogs
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        let snapshot = catalogs.get_mut(&key).ok_or(AgentSessionError::Failed)?;
        if !Arc::ptr_eq(&identity, &snapshot.identity) {
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
            snapshot.identity = Arc::new(());
            snapshot.fresh = false;
        }
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

    fn settings(&self, config: &StoredAgentConfig) -> Value {
        let mut features = config::features(config, config::ServiceTiers::default());
        // Synchronous settings have no catalog; render stored role keys only (no fixed set).
        features.extend(roles::stored_features(config));
        json!({"availableModes":config::modes(),"features":features,"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(super::configuration::executable(&self.program))
    }

    fn refresh_discovery_cache(&self, cwd: &str) {
        self.invalidate_role_catalog(cwd);
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let mut probe = session::probe(self, cwd, None, None).await?;
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
        let tiers = self
            .service_tiers(
                &spec.cwd,
                spec.config.model.as_deref(),
                spec.confinement.as_ref(),
            )
            .await?;
        let mut features = config::features(&spec.config, tiers);
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
        let mut probe = session::probe(self, &spec.cwd, None, spec.confinement.as_ref()).await?;
        let result = probe.request("get_available_commands", json!({})).await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        config::commands(&result)
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
        Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
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
        Ok(Box::new(session::open(self, spec, Some(handle)).await?) as Box<dyn AgentSession>)
    }
}

#[cfg(all(test, unix))]
mod tests;
