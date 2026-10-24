use crate::rpc::ErrorCode;
use model::realtime::Endpoint;

use super::AgentManager;

impl AgentManager {
    pub(crate) async fn start_realtime(
        &mut self,
        id: &str,
        endpoint: Endpoint,
    ) -> Result<bool, ErrorCode> {
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::InvalidMessage)?;
        if record.archived_at.is_some() || record.internal {
            return Err(ErrorCode::InvalidMessage);
        }
        let client = self
            .clients
            .get(&record.provider)
            .ok_or(ErrorCode::UnsupportedCapability)?;
        if client.settings(&record.config.clone().unwrap_or_default())["capabilities"]["supportsRealtimeVoice"]
            != true
        {
            return Ok(false);
        }
        if endpoint.cancel.is_cancelled() {
            return Err(ErrorCode::AgentIo);
        }
        self.resume(id).await.map_err(|_| ErrorCode::AgentIo)?;
        let agent = self.live.get_mut(id).ok_or(ErrorCode::AgentIo)?;
        if agent.turn.is_some() || agent.pending.is_some() || agent.session.realtime_active() {
            return Err(ErrorCode::AgentIo);
        }
        agent
            .session
            .start_realtime(endpoint)
            .await
            .map_err(|_| ErrorCode::AgentIo)?;
        Ok(true)
    }
}
