use domain::agent_runtime::{PersistedAgentRuntimeRecord, StoredAgentConfig};
use domain::bot::{ExecutionConfig, SessionOwner};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ErrorCode, ExecutionState, map_manager};
use crate::ports::agent_session::AgentSessionSpec;
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
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
                    self.refresh_bot_config(&record, instructions).await?;
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

    async fn refresh_bot_config(
        &self,
        record: &PersistedAgentRuntimeRecord,
        instructions: String,
    ) -> Result<(), ErrorCode> {
        let config = record.config.as_ref();
        let mode = bot_mode_override(
            &record.provider,
            config.and_then(|config| config.mode_id.as_deref()),
        );
        if mode.is_none()
            && config.and_then(|config| config.system_prompt.as_ref()) == Some(&instructions)
        {
            return Ok(());
        }
        self.manager
            .configure(
                &record.id,
                &ConfigPatch {
                    system_prompt: NullableSetting::Set(instructions),
                    mode_id: mode.map_or(NullableSetting::Unchanged, |mode| {
                        NullableSetting::Set(mode.into())
                    }),
                    ..ConfigPatch::default()
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
        mut config: ExecutionConfig,
        instructions: String,
    ) -> Result<Value, ErrorCode> {
        // Native auto-review can grant requests before they reach our permission callback.
        // Route new requests through Bot denial instead of native automatic reviewers.
        if let Some(mode) = bot_mode_override(&config.provider, config.mode_id.as_deref()) {
            config.mode_id = Some(mode.into());
        }
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

fn bot_mode_override(provider: &str, mode: Option<&str>) -> Option<&'static str> {
    match (provider, mode) {
        ("codex", Some("auto-review")) => Some("auto"),
        ("claude", Some("auto")) => Some("default"),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
