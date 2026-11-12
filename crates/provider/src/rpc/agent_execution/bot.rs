use domain::agent_runtime::{PersistedAgentRuntimeRecord, StoredAgentConfig};
use domain::bot::{ExecutionConfig, SessionOwner};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ErrorCode, ExecutionState, map_manager};
use crate::ports::agent_session::AgentSessionSpec;
use crate::ports::bot_session::{BotSessionSupport, InstructionDelivery};
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::protocol::prompt::AgentPrompt;
use crate::service::agent_manager::{AgentManagerError, AgentRegistration};
use crate::service::bot_execution::{Action, Created, TimelineCursor};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    owner: SessionOwner,
    operation: Action,
}

impl ExecutionState {
    pub(super) async fn bot(&mut self, params: Value) -> Result<Value, ErrorCode> {
        let request: Request =
            serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
        let existing = self.validate_bot(&request)?;
        match request.operation {
            Action::Create {
                agent_id,
                config,
                instructions,
            } => {
                self.open_bot_session(request.owner, existing, agent_id, config, instructions)
                    .await
            }
            Action::Send {
                agent_id,
                message_id,
                text,
            } => {
                self.send_bot(existing.is_some(), agent_id, message_id, text)
                    .await
            }
            Action::Poll { agent_id, after } => {
                if existing.is_none() {
                    return Err(ErrorCode::AgentNotFound);
                }
                let mut status = self.wait_result(&agent_id)?;
                let messages = self.manager.bot_messages(&agent_id, &after)?;
                status["messages"] =
                    serde_json::to_value(messages).map_err(|_| ErrorCode::AgentIo)?;
                Ok(status)
            }
            Action::Status { agent_id } => self.wait_result(&agent_id),
            Action::Release { agent_id } => {
                if self.manager.active_turn(&agent_id).is_some()
                    || self.manager.has_pending_input(&agent_id)?
                {
                    return Err(ErrorCode::CatalogBusy);
                }
                if existing.is_some() {
                    self.manager
                        .close(&agent_id)
                        .await
                        .map_err(|error| map_manager(&error))?;
                }
                Ok(json!({"released":true}))
            }
            Action::Cancel { agent_id } => {
                if existing.is_none() {
                    return Err(ErrorCode::AgentNotFound);
                }
                self.manager
                    .cancel(&agent_id)
                    .await
                    .map_err(|error| map_manager(&error))?;
                Ok(json!({"cancelled":true}))
            }
            Action::Close { agent_id } => {
                self.close_bot_session(&agent_id, existing.is_some())
                    .await?;
                Ok(json!({"closed":true}))
            }
            Action::Permission {
                agent_id,
                permission_id,
                response,
            } => {
                if existing.is_none() {
                    return Err(ErrorCode::AgentNotFound);
                }
                self.manager
                    .permission(&agent_id, &permission_id, &response)
                    .await?;
                Ok(json!({"resolved":true}))
            }
        }
    }

    /// Create the owner's session, or converge the existing one onto the provider's Bot profile.
    async fn open_bot_session(
        &mut self,
        owner: SessionOwner,
        existing: Option<PersistedAgentRuntimeRecord>,
        agent: String,
        config: ExecutionConfig,
        instructions: String,
    ) -> Result<Value, ErrorCode> {
        let support = self
            .manager
            .bot_session_support(
                existing
                    .as_ref()
                    .map_or(&config.provider, |record| &record.provider),
            )
            .map_err(|error| map_manager(&error))?;
        let profile = Profile::new(&support, instructions);
        let (created, reset_required) = if let Some(record) = existing {
            if record.archived_at.is_some() {
                return Err(ErrorCode::InvalidMessage);
            }
            (false, self.refresh_bot_config(&record, &profile).await?)
        } else {
            self.create_bot_session(owner, agent, config, profile)
                .await?;
            (true, false)
        };
        serde_json::to_value(Created {
            created,
            instruction_delivery: support.instructions,
            reset_required,
        })
        .map_err(|_| ErrorCode::AgentIo)
    }

    /// Converge an existing session onto `profile`; returns whether only a reset can.
    async fn refresh_bot_config(
        &self,
        record: &PersistedAgentRuntimeRecord,
        profile: &Profile,
    ) -> Result<bool, ErrorCode> {
        let Some(patch) = profile.patch(
            record
                .config
                .as_ref()
                .unwrap_or(&StoredAgentConfig::default()),
        ) else {
            return Ok(true);
        };
        if patch == ConfigPatch::default() {
            return Ok(false);
        }
        match self.manager.configure(&record.id, &patch).await {
            Ok(()) => Ok(false),
            Err(AgentManagerError::InvalidRequest) => Ok(true),
            Err(error) => Err(map_manager(&error)),
        }
    }

    fn validate_bot(
        &self,
        request: &Request,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, ErrorCode> {
        let agent = match &request.operation {
            Action::Create { agent_id, .. }
            | Action::Send { agent_id, .. }
            | Action::Poll { agent_id, .. }
            | Action::Status { agent_id }
            | Action::Cancel { agent_id }
            | Action::Close { agent_id }
            | Action::Release { agent_id }
            | Action::Permission { agent_id, .. } => agent_id,
        };
        let owner = serde_json::to_value(&request.owner).map_err(|_| ErrorCode::InvalidMessage)?;
        let existing = self.registry.get(agent).map_err(|_| ErrorCode::AgentIo)?;
        if existing.as_ref().is_some_and(|record| {
            !record.internal
                || record.owner.as_ref() != Some(&owner)
                || record.workspace_id.as_deref() != Some(&request.owner.workspace_id)
        }) {
            return Err(ErrorCode::InvalidMessage);
        }
        if !matches!(
            &request.operation,
            Action::Close { .. } | Action::Cancel { .. }
        ) {
            let workspace = self
                .workspaces
                .get(&request.owner.workspace_id)
                .map_err(|_| ErrorCode::RegistryIo)?
                .ok_or(ErrorCode::InvalidMessage)?;
            self.workspace(Some(&request.owner.workspace_id), &workspace.cwd)?;
        }
        Ok(existing)
    }
    async fn create_bot_session(
        &mut self,
        owner: SessionOwner,
        agent: String,
        config: ExecutionConfig,
        profile: Profile,
    ) -> Result<(), ErrorCode> {
        let workspace = self
            .workspaces
            .get(&owner.workspace_id)
            .map_err(|_| ErrorCode::RegistryIo)?
            .ok_or(ErrorCode::InvalidMessage)?;
        self.manager
            .create_with_environment(
                &agent,
                &AgentSessionSpec {
                    provider: config.provider,
                    cwd: workspace.cwd,
                    config: StoredAgentConfig {
                        model: config.model,
                        mode_id: profile.mode_id.map(str::to_owned),
                        thinking_option_id: config.thinking,
                        tool_policy: profile.tool_policy,
                        system_prompt: profile.system_prompt,
                        ..StoredAgentConfig::default()
                    },
                    confinement: None,
                },
                AgentRegistration {
                    workspace_id: Some(owner.workspace_id.clone()),
                    internal: true,
                    owner: Some(
                        serde_json::to_value(owner).map_err(|_| ErrorCode::InvalidMessage)?,
                    ),
                    ..AgentRegistration::default()
                },
                &crate::ports::environment::AgentEnvironment::default(),
            )
            .await
            .map_err(|error| map_manager(&error))?;
        Ok(())
    }

    async fn send_bot(
        &mut self,
        exists: bool,
        agent_id: String,
        message_id: String,
        text: String,
    ) -> Result<Value, ErrorCode> {
        if !exists {
            return Err(ErrorCode::AgentNotFound);
        }
        // Hydrate and reconcile the durable timeline before capturing the cursor. A resumed
        // session may replace its timeline generation while native history is being restored;
        // capturing the cursor first would make the following poll look stale and strand the
        // Bot turn as uncertain.
        if self.manager.timeline().is_some() {
            self.manager.load_timeline(&agent_id).await?;
        }
        let cursor = self
            .manager
            .timeline()
            .map(|timeline| {
                timeline
                    .read(&agent_id)
                    .map(|(epoch, rows)| TimelineCursor {
                        epoch,
                        seq: rows.last().map_or(0, |row| row.seq),
                    })
            })
            .transpose()?;
        self.manager
            .deliver(
                &agent_id,
                &AgentPrompt {
                    text,
                    client_message_id: Some(message_id),
                    ..AgentPrompt::default()
                },
                ActiveTurnBehavior::Interrupt,
            )
            .await?;
        let mut accepted = json!({"accepted":true});
        if let Some(cursor) = cursor {
            accepted["cursor"] = serde_json::to_value(cursor).map_err(|_| ErrorCode::AgentIo)?;
        }
        Ok(accepted)
    }

    async fn close_bot_session(&mut self, agent: &str, exists: bool) -> Result<(), ErrorCode> {
        if exists {
            let now = crate::support::time::now_timestamp();
            self.registry
                .update(agent, &|record| {
                    let mut record = record.clone();
                    record.archived_at = Some(now.clone());
                    record
                })
                .map_err(|_| ErrorCode::AgentIo)?;
            self.manager
                .reconcile()
                .await
                .map_err(|error| map_manager(&error))?;
        }
        Ok(())
    }
}

/// Native settings a Bot's own session must hold on its provider.
///
/// The Bot's `ExecutionConfig.mode_id` is ignored: the session runs in the provider's strictest
/// read-only mode, or its default mode where none exists, so writes that need approval are
/// denied automatically (ADR-145).
#[derive(Debug)]
struct Profile {
    mode_id: Option<&'static str>,
    tool_policy: Option<Value>,
    system_prompt: Option<String>,
}

impl Profile {
    fn new(support: &BotSessionSupport, instructions: String) -> Self {
        Self {
            mode_id: support.read_only_mode,
            tool_policy: support.tool_preapproval.then(preapproved_ait_tools),
            system_prompt: (support.instructions == InstructionDelivery::SystemPrompt)
                .then_some(instructions),
        }
    }

    /// The patch converging `current` onto this profile, or `None` when only a new session can.
    ///
    /// A mode cannot be cleared through configuration, and turn-prefix providers may reject
    /// prompt changes after opening, so their stored prompt is never patched.
    fn patch(&self, current: &StoredAgentConfig) -> Option<ConfigPatch> {
        let mode_id = if self.mode_id == current.mode_id.as_deref() {
            NullableSetting::Unchanged
        } else {
            NullableSetting::Set(self.mode_id?.to_owned())
        };
        let tool_policy = if self.tool_policy == current.tool_policy {
            NullableSetting::Unchanged
        } else {
            self.tool_policy
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set)
        };
        let system_prompt = match &self.system_prompt {
            Some(prompt) if current.system_prompt.as_ref() != Some(prompt) => {
                NullableSetting::Set(prompt.clone())
            }
            Some(_) | None => NullableSetting::Unchanged,
        };
        Some(ConfigPatch {
            mode_id,
            tool_policy,
            system_prompt,
            ..ConfigPatch::default()
        })
    }
}

/// Exact preapproval of every `ait` tool a Bot may ever be granted; MCP routing enforces the
/// Bot's current grants on each request, so settings changes never rewrite this policy.
fn preapproved_ait_tools() -> Value {
    let preapproved: Vec<Value> = domain::bot::permissions::grantable_tools()
        .map(|tool| json!({"kind":"mcp","server":crate::service::agent_mcp::SERVER,"tool":tool}))
        .collect();
    json!({"preapproved":preapproved})
}

#[cfg(test)]
mod tests;
