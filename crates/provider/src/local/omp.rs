//! Oh My Pi's native JSONL RPC adapter. OMP owns credentials, extensions, tools and session files.

mod actions;
mod bridge;
mod completion;
mod config;
mod framing;
mod history;
mod interactions;
mod roles;
mod session;
mod stream;
mod subagents;
mod transport;
mod ui;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture,
    AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::provider::Details;

#[derive(Debug, Clone)]
struct RoleSnapshot {
    catalog: roles::Catalog,
    default_details: Option<Details>,
    updated: Instant,
}

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
    ) -> Result<(roles::Catalog, (Option<String>, config::ServiceTiers)), AgentSessionError> {
        let _metadata = self.metadata_gate(&spec.cwd)?.lock_owned().await;
        let cached = self.cached_roles(&spec.cwd)?;
        let fresh = cached.is_none();
        let mut roles = match cached {
            Some(roles) => roles,
            None => roles::Catalog::read(self, &spec.cwd).await?,
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
                roles.read_chat(&mut probe).await?;
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
        roles: &roles::Catalog,
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

    fn cached_roles(&self, cwd: &str) -> Result<Option<roles::Catalog>, AgentSessionError> {
        let key = self.role_catalog_key(cwd);
        Ok(self
            .role_catalogs
            .read()
            .map_err(|_| AgentSessionError::Failed)?
            .get(&key)
            .filter(|snapshot| snapshot.updated.elapsed() < Duration::from_mins(5))
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

impl AgentClient for OmpClient {
    fn discovery_timeout(&self) -> Duration {
        // Config, model catalog and RPC each cold-start an official native process.
        // The catalog owner runs this in the background, outside all Agent lanes.
        transport::STARTUP_TIMEOUT.saturating_mul(3)
    }

    fn complete_commands<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        input: &'a crate::protocol::commands::CompletionInput,
    ) -> AgentSessionFuture<'a, crate::protocol::commands::CompletionResult> {
        Box::pin(async move {
            config::validate_spec(spec)?;
            input.validate()?;
            if *self
                .completion_support
                .read()
                .map_err(|_| AgentSessionError::Failed)?
                == Some(false)
            {
                return Ok(crate::protocol::commands::CompletionResult::default());
            }
            let mut probe = session::probe(self, &spec.cwd, spec.config.model.as_deref()).await?;
            let result = completion::complete(&mut probe, input).await;
            let closed = probe.close().await;
            let result = result?;
            closed?;
            Ok(result)
        })
    }
    fn supports_session_commands(&self) -> bool {
        true
    }

    fn provider(&self) -> &'static str {
        "omp"
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
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
        })
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

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(super::configuration::executable(&self.program)) })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
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
                None => roles::Catalog::read(self, cwd).await?,
            };
            let mut probe = session::probe(self, cwd, None).await?;
            roles.read_chat(&mut probe).await?;
            let result = config::discover(&mut probe).await;
            let closed = probe.close().await;
            let mut details = result?;
            closed?;
            roles.append(&mut details, &StoredAgentConfig::default());
            self.cache_roles(cwd, &roles, Some(&details))?;
            Ok(details)
        })
    }

    fn draft_features<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
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
                    .filter(|snapshot| snapshot.updated.elapsed() < Duration::from_mins(5))
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
        })
    }

    fn commands<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
            config::validate_spec(spec)?;
            let mut probe = session::probe(self, &spec.cwd, None).await?;
            let result = probe.request("get_available_commands", json!({})).await;
            let closed = probe.close().await;
            let result = result?;
            closed?;
            config::commands(&result)
        })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Ok(
                    "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN."
                        .to_owned(),
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
        })
    }

    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<crate::protocol::timeline::NativeItem>> {
        Box::pin(async move { Ok(history::inspect(self, handle, cwd).await?.entries) })
    }

    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, SessionHistory> {
        Box::pin(history::inspect(self, handle, cwd))
    }

    fn list_sessions<'a>(
        &'a self,
        options: &'a ListOptions,
    ) -> AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        Box::pin(history::list(self, options))
    }

    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            Ok(Box::new(bridge::Session::open(self, spec, None).await?) as Box<dyn AgentSession>)
        })
    }

    fn create_session_with_environment<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        environment: &'a AgentEnvironment,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
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
        Box::pin(async move {
            client.environment = AgentEnvironment::try_from(values)?;
            client.create_session(spec).await
        })
    }

    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            if purpose == AgentResumePurpose::History {
                return Err(AgentSessionError::Unavailable);
            }
            Ok(
                Box::new(bridge::Session::open(self, spec, Some(handle)).await?)
                    as Box<dyn AgentSession>,
            )
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
