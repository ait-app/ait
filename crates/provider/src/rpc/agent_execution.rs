//! Provider worker request decoding and response projections.

mod controls;
mod native_sessions;
mod placement;
mod resume;
mod scheduling;
mod voice;
/// Initial Agent validation and placement for the composite Workspace creation operation.
pub mod workspace_creation;
mod worktrees;

#[cfg(test)]
mod tests;

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::ports::agent_session::AgentSessionSpec;
use crate::protocol::agent_execution::{CreateRequest, ResumeRequest, SendRequest};
use crate::protocol::agent_lifecycle::AgentIdRequest;
use crate::rpc::ErrorCode;
use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
use crate::service::agent_runtime::AgentRuntimeDirectory;

pub(crate) struct ExecutionState {
    pub(crate) manager: AgentManager,
    pub(crate) message_observers:
        std::collections::BTreeMap<String, tokio::sync::watch::Receiver<Option<String>>>,
    pub(crate) owners: crate::service::agent_manager::ownership::Owners,
    pub(crate) directory: AgentRuntimeDirectory,
    pub(crate) registry: std::sync::Arc<dyn AgentRuntimeRegistry>,
    pub(crate) workspaces: std::sync::Arc<dyn WorkspaceRegistry>,
    pub(crate) projects: std::sync::Arc<dyn ProjectRegistry>,
    pub(crate) import_directory:
        Option<std::sync::Arc<dyn model::workspace::lifecycle::WorkspaceDirectory>>,
    pub(crate) workspace_automation:
        Option<std::sync::Arc<dyn model::workspace::lifecycle::WorkspaceSetup>>,
}

impl ExecutionState {
    pub(crate) async fn execute(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        match method {
            "internal.workspace.agent.create" => self.create_workspace_agent(params).await,
            "internal.workspace.retire" => self.retire_workspaces(params).await,
            "internal.agent.directory.prepare" => self.prepare_agent_directory(params),
            "internal.agent.identities.resolve" => {
                only(&params, &["agentIds"])?;
                let ids: Vec<String> = decode(params["agentIds"].clone())?;
                if ids.len() > 32 {
                    return Err(ErrorCode::InvalidMessage);
                }
                let ids = ids
                    .iter()
                    .map(|id| self.resolve(id))
                    .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
                Ok(json!(ids))
            }
            "agent.list.request" if params.get("sync").is_some_and(|sync| !sync.is_null()) => {
                self.synchronized_agents(params)
            }
            "internal.voice.send" | "internal.voice.status" | "internal.voice.cancel" => {
                self.voice(method, params).await
            }
            "agent.rewind.request"
            | "agent.commands.list.request"
            | "agent.permission.resolve.request"
            | "agent.provider_subagents.list.request"
            | "agent.provider_subagents.timeline.get.request"
            | "provider.diagnostic.request"
            | "provider.usage.list.request" => self.controls(method, params).await,
            "provider.sessions.recent.list.request"
            | "agent.import.request"
            | "agent.refresh.request"
            | "agent.fork_context.request" => self.native_sessions(method, params).await,
            "agent.create.request" => self.create(params).await,
            "provider.available.list.request"
            | "provider.models.list.request"
            | "provider.modes.list.request"
            | "provider.features.list.request"
            | "provider.snapshot.get.request"
            | "provider.snapshot.refresh.request" => self
                .manager
                .read_providers(method, params)
                .await
                .map_err(Into::into),
            "agent.timeline.get.request"
            | "agent.timeline.search.request"
            | "agent.timeline.list_prompts.request"
            | "internal.timeline.append" => self.timeline(method, params).await,
            "agent.resume.request" => self.resume(params).await,
            "agent.message.send.request" => self.send(params).await,
            "agent.model.set.request"
            | "agent.thinking.set.request"
            | "agent.config.apply.request"
            | "agent.mode.set.request"
            | "agent.feature.set.request" => self.configure(method, &params).await,
            "agent.cancel.request" => {
                only(&params, &["agentId"])?;
                let request: AgentIdRequest = decode(params)?;
                let id = self.resolve(&request.agent_id)?;
                self.manager
                    .cancel(&id)
                    .await
                    .map_err(|error| map_manager(&error))?;
                Ok(json!({"agentId":id,"agent":self.snapshot(&id)?,"error":null}))
            }
            "agent.finish.wait.request" => {
                let request: AgentIdRequest = decode(params)?;
                let id = self.resolve(&request.agent_id)?;
                self.wait_result(&id)
            }
            _ => {
                let result = super::agent_runtime::execute(&mut self.directory, method, params);
                // Archive may cascade and can partially succeed. Reconcile even on RPC failure.
                self.manager
                    .reconcile()
                    .await
                    .map_err(|error| map_manager(&error))?;
                let mut value = result?;
                self.decorate(&mut value)?;
                Ok(value)
            }
        }
    }

    async fn retire_workspaces(&mut self, params: Value) -> Result<Value, ErrorCode> {
        let workspaces: Vec<String> = decode(params)?;
        let archived = self
            .manager
            .archive_workspace_agents(&workspaces)
            .map_err(|error| map_manager(&error))?;
        self.manager
            .reconcile()
            .await
            .map_err(|error| map_manager(&error))?;
        Ok(json!(archived))
    }

    fn prepare_agent_directory(&self, params: Value) -> Result<Value, ErrorCode> {
        let mut listing = super::agent_runtime::listing::prepare(&self.directory, decode(params)?)?;
        for entry in &mut listing.entries {
            self.decorate(entry)?;
        }
        let projected: std::collections::BTreeMap<_, _> = listing
            .entries
            .iter()
            .filter_map(|entry| entry["agent"]["id"].as_str().map(|id| (id, entry)))
            .collect();
        if let Some(entries) = listing.response["entries"].as_array_mut() {
            for entry in entries {
                if let Some(projected) = entry["agent"]["id"]
                    .as_str()
                    .and_then(|id| projected.get(id))
                {
                    *entry = (*projected).clone();
                }
            }
        }
        listing.finish(&self.directory)
    }

    fn synchronized_agents(&self, params: Value) -> Result<Value, ErrorCode> {
        let request: crate::protocol::agent_lifecycle::AgentListRequest = decode(params)?;
        if request.subscribe.is_some() {
            return Err(ErrorCode::UnsupportedCapability);
        }
        let cursor = request.sync.clone().unwrap_or_default();
        let mut value = super::agent_runtime::sync_snapshot(&self.directory, request)?;
        self.decorate(&mut value)?;
        super::agent_runtime::synchronize(&self.directory, value, &cursor)
    }

    async fn timeline(&mut self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        use crate::protocol::timeline::{AppendRequest, FetchRequest, SearchRequest};
        let plugin = params
            .get("plugin")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let params = if method == "internal.timeline.append" {
            params["request"].clone()
        } else {
            params
        };
        let identifier = params["agentId"]
            .as_str()
            .ok_or(ErrorCode::InvalidMessage)?;
        let id = self.resolve(identifier)?;
        // A queued warm read may run after rewind invalidates the shared hydration marker.
        // Readers still use the committed projection; only the owner may start native hydration.
        if self.manager.owns_runtime(&id) {
            self.manager.load_timeline(&id).await?;
        }
        let timeline = self
            .manager
            .timeline()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        match method {
            "agent.timeline.get.request" => {
                let mut request: FetchRequest = decode(params)?;
                request.agent_id.clone_from(&id);
                let (epoch, rows) = timeline.read(&id)?;
                super::timeline::fetch(&request, &epoch, &rows, &self.snapshot(&id)?)
                    .map_err(Into::into)
            }
            "agent.timeline.search.request" => {
                let mut request: SearchRequest = decode(params)?;
                request.agent_id = id;
                let (epoch, rows) = timeline.read(&request.agent_id)?;
                super::timeline::search(&request, &epoch, &rows).map_err(Into::into)
            }
            "agent.timeline.list_prompts.request" => {
                only(&params, &["agentId"])?;
                let (epoch, rows) = timeline.read(&id)?;
                super::timeline::prompts(&id, &epoch, &rows).map_err(Into::into)
            }
            "internal.timeline.append" => {
                let plugin = plugin.ok_or(ErrorCode::UnsupportedCapability)?;
                let request: AppendRequest = decode(params)?;
                if request.item.r#type != "plugin"
                    || request.item.version == 0
                    || !model::valid_id(&request.item.id)
                    || !model::valid_id(&request.item.kind)
                    || serde_json::to_vec(&request.item.data)
                        .map_err(|_| ErrorCode::InvalidMessage)?
                        .len()
                        > 65536
                {
                    return Err(ErrorCode::InvalidMessage);
                }
                let key = format!("plugin:{plugin}:{}", request.item.id);
                let mut item =
                    serde_json::to_value(request.item).map_err(|_| ErrorCode::InvalidMessage)?;
                item["pluginId"] = json!(plugin);
                let entry = crate::protocol::timeline::NativeItem {
                    key,
                    turn_id: None,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    item,
                };
                let provider = self
                    .registry
                    .get(&id)
                    .map_err(|_| ErrorCode::AgentIo)?
                    .ok_or(ErrorCode::AgentNotFound)?
                    .provider;
                let (epoch, positions) = timeline.append(&id, &provider, &[entry])?;
                Ok(json!({"epoch":epoch,"seq":positions.first()}))
            }
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    async fn create(&mut self, params: Value) -> Result<Value, ErrorCode> {
        let (mut request, intent) = parse_creation(params)?;
        self.manager
            .validate_provider(&request.config.provider)
            .map_err(|error| map_manager(&error))?;
        if let Some(id) = &request.agent_id {
            Uuid::parse_str(id).map_err(|_| ErrorCode::InvalidMessage)?;
        }
        let key = request
            .idempotency_key
            .take()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let creations = self.manager.creations();
        let admission = creations.begin(domain::creation::protocol::Kind::Agent, &key, intent)?;
        if !admission.execute {
            if admission.snapshot.phase == "completed"
                && let Some(id) = &admission.snapshot.agent_id
                && !self
                    .directory
                    .contains_identity(id)
                    .map_err(|_| ErrorCode::AgentIo)?
            {
                return Err(ErrorCode::AgentNotFound);
            }
            return Ok(
                json!({"agentId":admission.snapshot.agent_id,"agent":admission.snapshot.agent,"error":admission.snapshot.error,"creation":admission.snapshot}),
            );
        }
        if let Some(id) = &admission.snapshot.agent_id
            && self
                .directory
                .contains_identity(id)
                .map_err(|_| ErrorCode::AgentIo)?
        {
            creations.advance(
                &admission.snapshot,
                "failed",
                None,
                Some("Agent ID is already in use".into()),
            )?;
            return Err(ErrorCode::IdempotencyConflict);
        }
        let (workspace_id, created_worktree) = match self.creation_placement(&mut request).await {
            Ok(placement) => placement,
            Err(error) => {
                creations.advance(&admission.snapshot, "failed", None, Some(error.to_string()))?;
                return Err(error);
            }
        };
        self.register_creation(request, &admission.snapshot, workspace_id, created_worktree)
            .await
    }

    async fn register_creation(
        &mut self,
        request: CreateRequest,
        admission: &domain::creation::protocol::Snapshot,
        workspace_id: String,
        created_worktree: bool,
    ) -> Result<Value, ErrorCode> {
        self.manager.place_workspace(&workspace_id)?;
        let creations = self.manager.creations();
        let id = match admission.agent_id.clone() {
            Some(id) => {
                Uuid::parse_str(&id).map_err(|_| ErrorCode::InvalidMessage)?;
                id
            }
            None => Uuid::new_v4().to_string(),
        };
        let created = self
            .manager
            .create_with_environment(
                &id,
                &AgentSessionSpec {
                    provider: request.config.provider,
                    cwd: request.config.cwd,
                    config: request.config.stored,
                },
                AgentRegistration {
                    workspace_id: Some(workspace_id.clone()),
                    title: request.config.title.map(|title| title.trim().to_owned()),
                    labels: request.labels,
                    internal: false,
                },
                &request.env,
            )
            .await;
        let created = created.and_then(|record| {
            self.workspace(Some(&workspace_id), &record.cwd)
                .map_err(|_| AgentManagerError::InvalidRequest)?;
            if let Some(parent) = record.labels.get("paseo.parent-agent-id")
                && self
                    .registry
                    .get(parent)
                    .map_err(|_| AgentManagerError::Registry)?
                    .is_none_or(|parent| parent.archived_at.is_some())
            {
                return Err(AgentManagerError::InvalidRequest);
            }
            Ok(record)
        });
        if let Err(error) = created {
            if self.manager.live_snapshot(&id).is_some() {
                self.directory
                    .archive(&id, &chrono::Utc::now().to_rfc3339())
                    .map_err(|_| ErrorCode::RegistryIo)?;
                self.manager
                    .reconcile()
                    .await
                    .map_err(|error| map_manager(&error))?;
            }
            let cleanup_failed = created_worktree
                && self
                    .cleanup_created_worktree(&id, &workspace_id)
                    .await
                    .is_err();
            let message = if cleanup_failed {
                format!("{error}; worktree cleanup failed for Workspace {workspace_id}")
            } else {
                error.to_string()
            };
            creations.advance(admission, "failed", None, Some(message))?;
            return Err(map_manager(&error));
        }
        if created_worktree {
            self.start_worktree_setup(&workspace_id).await;
        }
        if request.auto_archive {
            self.arm_auto_archive(&id, &workspace_id, created_worktree)?;
        }
        let prompt = (request.initial_prompt.is_some()
            || !request.images.is_empty()
            || !request.attachments.is_empty())
        .then(|| crate::protocol::prompt::AgentPrompt {
            text: request.initial_prompt.unwrap_or_default(),
            images: request.images,
            attachments: request.attachments,
            client_message_id: request.client_message_id,
            output_schema: request.output_schema,
        });
        self.finish_creation(&id, admission, prompt).await
    }

    async fn finish_creation(
        &mut self,
        id: &str,
        admission: &domain::creation::protocol::Snapshot,
        prompt: Option<crate::protocol::prompt::AgentPrompt>,
    ) -> Result<Value, ErrorCode> {
        let creations = self.manager.creations();
        let result = json!({"agent":self.snapshot(id)?});
        let mut progress = creations.advance(admission, "agent_ready", Some(result), None)?;
        if let Some(prompt) = prompt {
            if let Err(error) = self.manager.send_input(id, &prompt).await {
                creations.advance(&progress, "failed", None, Some(error.to_string()))?;
                return Err(map_manager(&error));
            }
            progress = creations.advance(&progress, "prompt_started", None, None)?;
        }
        let snapshot = self.snapshot(id)?;
        progress = creations.advance(
            &progress,
            "completed",
            Some(json!({"agent":snapshot})),
            None,
        )?;
        Ok(
            json!({"status":"agent_created","agentId":id,"agent":snapshot,"error":null,"creation":progress}),
        )
    }

    async fn send(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(
            &params,
            &[
                "agentId",
                "text",
                "activeTurnBehavior",
                "messageId",
                "images",
                "attachments",
                "outputSchema",
            ],
        )?;
        let request: SendRequest = decode(params)?;
        let id = self.resolve(&request.agent_id)?;
        let record = self
            .registry
            .get(&id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        self.workspace(record.workspace_id.as_deref(), &record.cwd)?;
        let behavior = request.active_turn_behavior.unwrap_or_default();
        let prompt = request.into_prompt();
        let result = self.manager.deliver(&id, &prompt, behavior).await;
        Ok(
            json!({"agentId":id,"accepted":result.is_ok(),"error":result.err().map(model::ErrorCode::message)}),
        )
    }

    pub(crate) fn workspace(&self, selected: Option<&str>, cwd: &str) -> Result<String, ErrorCode> {
        let canonical = std::fs::canonicalize(cwd).map_err(|_| ErrorCode::InvalidMessage)?;
        let workspace = if let Some(id) = selected {
            self.workspaces.get(id).map_err(|_| ErrorCode::RegistryIo)?
        } else {
            self.workspaces
                .list()
                .map_err(|_| ErrorCode::RegistryIo)?
                .into_iter()
                .filter(|workspace| {
                    workspace.archived_at.as_ref().is_none_or(String::is_empty)
                        && std::fs::canonicalize(&workspace.cwd).ok().as_ref() == Some(&canonical)
                })
                .min_by(|left, right| {
                    left.created_at
                        .cmp(&right.created_at)
                        .then(left.workspace_id.cmp(&right.workspace_id))
                })
        }
        .ok_or(ErrorCode::InvalidMessage)?;
        let project = self
            .projects
            .get(&workspace.project_id)
            .map_err(|_| ErrorCode::RegistryIo)?
            .ok_or(ErrorCode::InvalidMessage)?;
        if workspace
            .archived_at
            .as_ref()
            .is_some_and(|value| !value.is_empty())
            || project
                .archived_at
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            || (selected.is_none()
                && std::fs::canonicalize(&workspace.cwd).ok().as_ref() != Some(&canonical))
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(workspace.workspace_id)
    }

    pub(crate) fn resolve(&self, identifier: &str) -> Result<String, ErrorCode> {
        self.directory
            .get(identifier)
            .map(|resolved| resolved.agent.id)
            .map_err(|error| match error {
                crate::service::agent_runtime::AgentRuntimeError::NotFound(_) => {
                    ErrorCode::AgentNotFound
                }
                crate::service::agent_runtime::AgentRuntimeError::Ambiguous(_)
                | crate::service::agent_runtime::AgentRuntimeError::InvalidRequest => {
                    ErrorCode::InvalidMessage
                }
                crate::service::agent_runtime::AgentRuntimeError::AgentRegistry => {
                    ErrorCode::AgentIo
                }
                crate::service::agent_runtime::AgentRuntimeError::WorkspaceRegistry => {
                    ErrorCode::RegistryIo
                }
            })
    }

    fn snapshot(&self, id: &str) -> Result<Value, ErrorCode> {
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        self.project(&record)
    }

    fn project(&self, record: &PersistedAgentRuntimeRecord) -> Result<Value, ErrorCode> {
        let mut snapshot = serde_json::to_value(super::agent_runtime::snapshot(record))
            .map_err(|_| ErrorCode::AgentIo)?;
        snapshot["lastError"] = json!(record.last_error);
        if self.manager.live_snapshot(&record.id).is_some() {
            snapshot["providerUnavailable"] = json!(false);
            snapshot["persistence"] = json!(record.persistence);
            // Explicitly retire the client's previous turn when the native writer is idle.
            snapshot["activeTurn"] = Value::Null;
            if let Some(turn) = self.manager.active_turn(&record.id) {
                snapshot["activeTurn"] =
                    json!({"turnId":turn,"startedAt":record.last_user_message_at});
            }
        }
        self.manager.control_snapshot(record, &mut snapshot);
        if !self.manager.owns_runtime(&record.id)
            && let Some(committed) = self.owners.snapshot(&record.id)
        {
            for field in [
                "status",
                "requiresAttention",
                "attentionReason",
                "lastError",
                "activeTurn",
                "providerUnavailable",
                "pendingPermissions",
            ] {
                if let Some(value) = committed.get(field) {
                    snapshot[field] = value.clone();
                }
            }
        }
        Ok(snapshot)
    }

    fn decorate(&self, value: &mut Value) -> Result<(), ErrorCode> {
        // Only replace snapshots at known response positions; arbitrary provider JSON is opaque.
        if let Some(snapshot) = value.get_mut("agent").filter(|value| value.is_object())
            && let Some(id) = snapshot.get("id").and_then(Value::as_str)
        {
            *snapshot = self.snapshot(id)?;
        }
        if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                self.decorate(entry)?;
            }
        }
        if let Some(agents) = value.get_mut("agents").and_then(Value::as_array_mut) {
            for agent in agents {
                if let Some(id) = agent.get("id").and_then(Value::as_str) {
                    *agent = self.snapshot(id)?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn only(params: &Value, allowed: &[&str]) -> Result<(), ErrorCode> {
    let object = params.as_object().ok_or(ErrorCode::InvalidMessage)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)
}

const fn map_manager(error: &AgentManagerError) -> ErrorCode {
    match error {
        AgentManagerError::NotFound(_) => ErrorCode::AgentNotFound,
        AgentManagerError::ProviderUnavailable(_)
        | AgentManagerError::MissingPersistence(_)
        | AgentManagerError::Busy => ErrorCode::UnsupportedCapability,
        AgentManagerError::InvalidRequest
        | AgentManagerError::AlreadyExists(_)
        | AgentManagerError::SessionRejected => ErrorCode::InvalidMessage,
        AgentManagerError::Session | AgentManagerError::Registry => ErrorCode::AgentIo,
    }
}

fn parse_creation(params: Value) -> Result<(CreateRequest, Value), ErrorCode> {
    only(
        &params,
        &[
            "agentId",
            "config",
            "workspaceId",
            "callerAgentId",
            "autoArchive",
            "env",
            "worktree",
            "git",
            "worktreeName",
            "labels",
            "idempotencyKey",
            "subscribe",
            "initialPrompt",
            "images",
            "attachments",
            "clientMessageId",
            "outputSchema",
        ],
    )?;
    only(
        &params["config"],
        &[
            "provider",
            "cwd",
            "title",
            "modeId",
            "model",
            "thinkingOptionId",
            "systemPrompt",
            "featureValues",
            "providerOptions",
            "mcpServers",
            "toolPolicy",
        ],
    )?;
    let mut intent = params.clone();
    if let Some(object) = intent.as_object_mut() {
        object.remove("idempotencyKey");
        object.remove("subscribe");
        if let Some(environment) = object.get_mut("env") {
            use sha2::Digest;
            // Creation receipts must bind retries without retaining environment secrets.
            *environment = json!({"sha256":format!("{:x}", sha2::Sha256::digest(environment.to_string().as_bytes()))});
        }
    }
    let request: CreateRequest = decode(params)?;
    worktrees::intent(&request)?;
    if request.initial_prompt.is_some()
        || !request.images.is_empty()
        || !request.attachments.is_empty()
    {
        crate::protocol::prompt::AgentPrompt {
            text: request.initial_prompt.clone().unwrap_or_default(),
            images: request.images.clone(),
            attachments: request.attachments.clone(),
            client_message_id: request.client_message_id.clone(),
            output_schema: request.output_schema.clone(),
        }
        .validate()
        .map_err(|_| ErrorCode::InvalidMessage)?;
    }
    if request
        .config
        .title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty() || title.trim().encode_utf16().count() > 200)
        || request.labels.len() > 100
        || request
            .labels
            .iter()
            .any(|(key, value)| key.len() > 256 || value.len() > 4096)
    {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok((request, intent))
}
