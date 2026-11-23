//! Apply a library preset without blocking the Agent's permission/event lane.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{ErrorCode, ExecutionState, only};
use crate::service::agent_manager::preset_selection::PreparedPreset;

pub(crate) struct Pending {
    id: String,
    started: Instant,
    result: oneshot::Receiver<Result<PreparedPreset, model::ErrorCode>>,
    reply: oneshot::Sender<Result<Value, ErrorCode>>,
}

impl ExecutionState {
    pub(crate) fn begin_preset_selection(
        &mut self,
        params: &Value,
        reply: oneshot::Sender<Result<Value, ErrorCode>>,
    ) {
        let result = (|| {
            only(params, &["agentId", "name", "source"])?;
            let id = self.resolve_field(params, "agentId")?;
            self.active_workspace(&id)?;
            self.guard_observed_request("agent.model_preset.select.request", params)?;
            let name = params["name"].as_str().ok_or(ErrorCode::InvalidMessage)?;
            let source = params["source"].as_str().ok_or(ErrorCode::InvalidMessage)?;
            if self.preset_lookups.len() >= 4 {
                return Err(ErrorCode::CatalogBusy);
            }
            let pending = self
                .manager
                .prepare_preset(&id, name, source, None)
                .map_err(ErrorCode::from)?;
            Ok((id, pending))
        })();
        match result {
            Ok((id, result)) => self.preset_lookups.push(Pending {
                id,
                started: Instant::now(),
                result,
                reply,
            }),
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    pub(crate) fn settle_preset_selections(&mut self) {
        let mut index = 0;
        while index < self.preset_lookups.len() {
            let pending = &mut self.preset_lookups[index];
            let result = match pending.result.try_recv() {
                Ok(result) => Some(result.map_err(ErrorCode::from)),
                Err(oneshot::error::TryRecvError::Closed) => Some(Err(ErrorCode::AgentIo)),
                Err(oneshot::error::TryRecvError::Empty)
                    if pending.started.elapsed() >= Duration::from_secs(60) =>
                {
                    Some(Err(ErrorCode::AgentIo))
                }
                Err(oneshot::error::TryRecvError::Empty) => None,
            };
            let Some(result) = result else {
                index += 1;
                continue;
            };
            let pending = self.preset_lookups.swap_remove(index);
            let result = result.and_then(|prepared| self.manager.commit_preset(&prepared).map_err(ErrorCode::from))
                .map(|()| json!({"agentId":pending.id,"accepted":true,"error":null,
                    "notice":self.manager.active_turn(&pending.id).map(|_| json!({"type":"warning","message":"Configuration applies next turn"}))}));
            let _ = self.publish();
            let _ = pending.reply.send(result);
        }
    }
}

impl ExecutionState {
    /// List the Agent's authoritative preset scope without exposing its host directory.
    pub(crate) async fn model_preset_context(&self, params: &Value) -> Result<Value, ErrorCode> {
        only(params, &["agentId"])?;
        let id = self.resolve_field(params, "agentId")?;
        self.active_workspace(&id)?;
        self.manager
            .model_preset_context(&id)
            .await
            .map_err(ErrorCode::from)
    }
}
