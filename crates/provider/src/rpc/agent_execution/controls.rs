//! Native controls serialized with turns and configuration updates.

use serde_json::{Value, json};

use super::{ErrorCode, ExecutionState, decode, only};
use crate::ports::agent_session::AgentSessionSpec;
use crate::protocol::agent_config::ConfigPatch;
use crate::protocol::controls::{
    CommandsRequest, PermissionRequest, RewindRequest, SubagentRequest,
};
use crate::protocol::timeline::{FetchRequest, Projection};
use crate::service::agent_execution::operation::Operation;
use crate::service::agent_manager::native_sessions::canonical;

impl ExecutionState {
    pub(super) async fn unified_subagents(&self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["parentAgentId", "subagentId"])?;
        let parent = self.resolve(
            params["parentAgentId"]
                .as_str()
                .ok_or(ErrorCode::InvalidMessage)?,
        )?;
        let record = self
            .registry
            .get(&parent)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        let root = record
            .observation
            .as_ref()
            .map_or(parent.as_str(), |source| source.root_agent_id.as_str());
        // Providers without child discovery can still have Ait-managed descendants.
        if self
            .registry
            .get(root)
            .map_err(|_| ErrorCode::AgentIo)?
            .is_some_and(|record| record.persistence.is_some() && record.archived_at.is_none())
        {
            let _ = self.manager.subagents(root).await;
        }
        if let Some(native) = params.get("subagentId") {
            let native = native.as_str().ok_or(ErrorCode::InvalidMessage)?;
            let id = self
                .manager
                .timeline()
                .ok_or(ErrorCode::UnsupportedCapability)?
                .subagent_agent(root, native)?
                .ok_or(ErrorCode::AgentNotFound)?;
            let record = self
                .registry
                .get(&id)
                .map_err(|_| ErrorCode::AgentIo)?
                .ok_or(ErrorCode::AgentNotFound)?;
            if record.internal
                || record
                    .observation
                    .as_ref()
                    .is_some_and(|source| !source.ready)
            {
                return Err(ErrorCode::AgentNotFound);
            }
            return Ok(
                json!({"parentAgentId":parent,"agents":[self.project(&record)?],"error":null}),
            );
        }
        let agents = self
            .registry
            .list()
            .map_err(|_| ErrorCode::AgentIo)?
            .iter()
            .filter(|record| !record.internal && record.archived_at.is_none())
            .filter(|record| {
                record.observation.as_ref().map_or_else(
                    || {
                        record
                            .labels
                            .get("paseo.parent-agent-id")
                            .is_some_and(|id| id == &parent)
                    },
                    |source| source.ready && source.parent_agent_id == parent,
                )
            })
            .map(|record| self.project(record))
            .collect::<Result<Vec<_>, _>>()?;
        crate::rpc::timeline::bounded(json!({"parentAgentId":parent,"agents":agents,"error":null}))
            .map_err(Into::into)
    }

    pub(super) async fn diagnostic(&self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["provider"])?;
        let provider = params["provider"]
            .as_str()
            .ok_or(ErrorCode::InvalidMessage)?;
        self.manager.diagnostic(provider).await.map_err(Into::into)
    }

    pub(super) async fn usage(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId", "providerId", "forceRefresh"])?;
        let agent = match params.get("agentId") {
            None => None,
            Some(value) => Some(self.resolve(value.as_str().ok_or(ErrorCode::InvalidMessage)?)?),
        };
        let force = match params.get("forceRefresh") {
            None => false,
            Some(value) => value.as_bool().ok_or(ErrorCode::InvalidMessage)?,
        };
        let provider = match params.get("providerId") {
            None => None,
            Some(value) => Some(
                value
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 256)
                    .ok_or(ErrorCode::InvalidMessage)?,
            ),
        };
        self.manager
            .usage(agent.as_deref(), provider, force)
            .await
            .map_err(Into::into)
    }

    pub(super) async fn rewind(&mut self, params: Value) -> Result<Value, ErrorCode> {
        let request: RewindRequest = decode(params)?;
        let id = self.resolve(&request.agent_id)?;
        self.active_workspace(&id)?;
        let result = if request.message_id.is_empty() || request.message_id.len() > 512 {
            Err(model::ErrorCode::InvalidMessage)
        } else {
            self.manager
                .rewind_mode(&id, &request.message_id, &request.mode)
                .await
        };
        Ok(
            json!({"agentId":id,"ok":result.is_ok(),"error":result.err().map(|error|format!("{error:?}"))}),
        )
    }

    pub(super) async fn permission(&mut self, params: Value) -> Result<Value, ErrorCode> {
        let request: PermissionRequest = decode(params)?;
        let id = self.resolve(&request.agent_id)?;
        self.active_workspace(&id)?;
        self.manager
            .permission(&id, &request.request_id, &request.response)
            .await?;
        Ok(json!({"agentId":id,"requestId":request.request_id,"resolution":request.response}))
    }

    /// Apply one configuration request whose payload is carried in `field`: `config` for a
    /// bundle, `value` for a feature, or the named single configuration field.
    pub(super) async fn configure(
        &self,
        field: &'static str,
        params: &Value,
    ) -> Result<Value, ErrorCode> {
        let allowed = if field == "value" {
            vec!["agentId", "featureId", "value"]
        } else {
            vec!["agentId", field]
        };
        only(params, &allowed)?;
        let id = self.resolve(
            params["agentId"]
                .as_str()
                .ok_or(ErrorCode::InvalidMessage)?,
        )?;
        self.active_workspace(&id)?;
        let value = params.get(field).ok_or(ErrorCode::InvalidMessage)?;
        let patch = match field {
            "config" => {
                only(
                    value,
                    &[
                        "modelId",
                        "thinkingOptionId",
                        "modeId",
                        "featureValues",
                        "providerOptions",
                        "mcpServers",
                        "toolPolicy",
                        "systemPrompt",
                    ],
                )?;
                value.clone()
            }
            "value" => {
                let feature = params["featureId"]
                    .as_str()
                    .ok_or(ErrorCode::InvalidMessage)?;
                json!({"featureValues":{(feature):value}})
            }
            _ => json!({(field):value}),
        };
        if patch
            .get("featureValues")
            .is_some_and(|value| !value.is_object())
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let patch: ConfigPatch = decode(patch)?;
        let result = self.manager.configure(&id, &patch).await;
        let notice = (result.is_ok() && self.manager.active_turn(&id).is_some())
            .then(|| json!({"type":"warning","message":"Configuration applies next turn"}));
        Ok(
            json!({"agentId":id,"accepted":result.is_ok(),"error":result.err().map(|error|error.to_string()),"notice":notice}),
        )
    }

    pub(super) async fn commands(&self, params: Value) -> Result<Value, ErrorCode> {
        if let Some(config) = params.get("draftConfig").filter(|value| !value.is_null()) {
            only(
                config,
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
        }
        let request: CommandsRequest = decode(params)?;
        let (id, spec) = match self.resolve(&request.agent_id) {
            Ok(id) => {
                let record = self
                    .registry
                    .get(&id)
                    .map_err(|_| ErrorCode::AgentIo)?
                    .ok_or(ErrorCode::AgentNotFound)?;
                (
                    id,
                    AgentSessionSpec {
                        provider: record.provider,
                        cwd: record.cwd,
                        config: record.config.unwrap_or_default(),
                    },
                )
            }
            Err(ErrorCode::AgentNotFound) => {
                let draft = request.draft_config.ok_or(ErrorCode::AgentNotFound)?;
                (
                    request.agent_id,
                    AgentSessionSpec {
                        provider: draft.provider,
                        cwd: canonical(&draft.cwd)?,
                        config: draft.stored,
                    },
                )
            }
            Err(error) => return Err(error),
        };
        let commands = self.manager.commands(&spec).await?;
        crate::rpc::timeline::bounded(json!({"agentId":id,"commands":commands,"error":null}))
            .map_err(Into::into)
    }

    pub(super) async fn subagents(
        &self,
        method: Operation,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        let list = method == Operation::Subagents;
        if list {
            only(&params, &["parentAgentId"])?;
        }
        let request: SubagentRequest = decode(params)?;
        let id = self.resolve(&request.parent_agent_id)?;
        let record = self
            .registry
            .get(&id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        let children = self.manager.subagents(&id).await?;
        if list {
            let root = record
                .persistence
                .as_ref()
                .ok_or(ErrorCode::UnsupportedCapability)?;
            let subagents: Vec<_> = children
                .into_iter()
                .map(|child| {
                    let mut value = child.descriptor;
                    value["parentAgentId"] = json!(id);
                    value["parentSubagentId"] =
                        json!((child.parent_id != root.session_id).then_some(child.parent_id));
                    value
                })
                .collect();
            return crate::rpc::timeline::bounded(
                json!({"parentAgentId":id,"subagents":subagents,"error":null}),
            )
            .map_err(Into::into);
        }
        let child_id = request.subagent_id.ok_or(ErrorCode::InvalidMessage)?;
        let child = children
            .iter()
            .find(|child| child.id == child_id)
            .ok_or(ErrorCode::AgentNotFound)?;
        let timeline = self
            .manager
            .timeline()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let scope = format!("subagent:{id}:{child_id}");
        let existing = timeline.has_entries(&scope)?;
        let live = self.manager.live_child(&id, &child_id);
        if !live || !existing {
            match self.manager.child_history(&id, child).await {
                Ok(history) => {
                    timeline.reconcile(&scope, &record.provider, &history.entries)?;
                }
                Err(_) if live || existing => {}
                Err(error) => return Err(error.into()),
            }
        }
        let fetch = FetchRequest {
            agent_id: id.clone(),
            direction: request.direction,
            cursor: request.cursor,
            limit: request.limit,
            projection: Projection::Projected,
            merge_window: None,
            allow_cached: None,
        };
        let mut value =
            crate::rpc::timeline::fetch_stored(&fetch, &timeline, &scope, &Value::Null, None)?;
        let object = value.as_object_mut().ok_or(ErrorCode::AgentIo)?;
        object.remove("agent");
        object.remove("agentId");
        let mut entries = object.remove("entries").ok_or(ErrorCode::AgentIo)?;
        if let Some(entries) = entries.as_array_mut() {
            for entry in entries {
                entry["seq"] = entry["seqStart"].clone();
            }
        }
        object.insert("rows".to_owned(), entries);
        object.insert("parentAgentId".to_owned(), json!(id));
        object.insert("subagentId".to_owned(), json!(child_id));
        object.insert("provider".to_owned(), json!(record.provider));
        crate::rpc::timeline::bounded(value).map_err(Into::into)
    }

    fn active_workspace(&self, id: &str) -> Result<(), ErrorCode> {
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        self.workspace(record.workspace_id.as_deref(), &record.cwd)?;
        Ok(())
    }
}
