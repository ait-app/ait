use domain::agent_runtime::{PersistedAgentRuntimeRecord, StoredAgentConfig};
use domain::bot::{ExecutionConfig, SessionOwner};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ErrorCode, ExecutionState, map_manager};
use crate::ports::agent_session::AgentSessionSpec;
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::protocol::prompt::AgentPrompt;
use crate::service::agent_manager::AgentRegistration;
use crate::service::bot_execution::Action;

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
                if let Some(record) = existing {
                    if record.archived_at.is_some() {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    self.refresh_bot_instructions(&record, instructions).await?;
                    return Ok(json!({"created":false}));
                }
                self.create_bot_session(request.owner, agent_id, config, instructions)
                    .await
            }
            Action::Send {
                agent_id,
                message_id,
                text,
            } => {
                if existing.is_none() {
                    return Err(ErrorCode::AgentNotFound);
                }
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
                Ok(json!({"accepted":true}))
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

    async fn refresh_bot_instructions(
        &self,
        record: &PersistedAgentRuntimeRecord,
        instructions: String,
    ) -> Result<(), ErrorCode> {
        if record
            .config
            .as_ref()
            .and_then(|config| config.system_prompt.as_ref())
            == Some(&instructions)
        {
            return Ok(());
        }
        self.manager
            .configure(
                &record.id,
                &crate::protocol::agent_config::ConfigPatch {
                    system_prompt: crate::protocol::agent_config::NullableSetting::Set(
                        instructions,
                    ),
                    ..crate::protocol::agent_config::ConfigPatch::default()
                },
            )
            .await
            .map_err(|error| map_manager(&error))
    }

    fn validate_bot(
        &self,
        request: &Request,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, ErrorCode> {
        let agent = match &request.operation {
            Action::Create { agent_id, .. }
            | Action::Send { agent_id, .. }
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
        instructions: String,
    ) -> Result<Value, ErrorCode> {
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
                        mode_id: config.mode_id,
                        thinking_option_id: config.thinking,
                        system_prompt: Some(instructions),
                        ..StoredAgentConfig::default()
                    },
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
        Ok(json!({"created":true}))
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
