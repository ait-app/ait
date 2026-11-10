//! BTW promotion uses full native records, never the bounded client projection.

use super::Session;
use crate::ports::agent_session::{AgentSessionError, AgentSessionFuture};
use serde_json::{Value, json};
use uuid::Uuid;

impl Session {
    /// Admit promotion only for a completed single-turn topic at the current idle leaf.
    pub(super) async fn begin_btw_branch(
        &mut self,
        fields: &Value,
    ) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
        self.actions.check_capacity()?;
        if self.btw.running() {
            return Err(AgentSessionError::Busy);
        }
        if !self
            .native_commands
            .iter()
            .any(|entry| entry["name"] == "ait-btw-branch-v1" && entry["source"] == "extension")
        {
            return Err(AgentSessionError::Unavailable);
        }
        // These reads are serialized on the current owner. The extension checks session and
        // leaf again immediately before the public session transition.
        let history = self.transport.request("get_btw_history", json!({})).await?;
        let state = self.transport.request("get_state", json!({})).await?;
        let entries = self.transport.request("get_entries", json!({})).await?;
        let record = history["records"]
            .as_array()
            .and_then(|records| {
                records
                    .iter()
                    .find(|record| record["id"] == fields["recordId"])
            })
            .ok_or(AgentSessionError::Rejected)?;
        if state["isStreaming"] == true
            || state["isCompacting"] == true
            || state["hasPendingAsyncWork"] == true
            || state["queuedMessageCount"].as_u64().is_some_and(|n| n > 0)
            || state["isSettled"] == false
        {
            return Err(AgentSessionError::Busy);
        }
        if record["status"] != "complete"
            || record["followUps"]
                .as_array()
                .is_some_and(|turns| !turns.is_empty())
            || !record["leafId"].is_string()
            || record["leafId"] != entries["leafId"]
            || state["sessionId"] != self.state["sessionId"]
            || record["question"]
                .as_str()
                .is_none_or(|text| text.trim().is_empty())
            || record["answer"]
                .as_str()
                .is_none_or(|text| text.trim().is_empty())
            || record["images"]
                .as_array()
                .is_some_and(|images| !images.is_empty())
        {
            return Err(AgentSessionError::Rejected);
        }
        let nonce = Uuid::new_v4().to_string();
        let arguments = json!({"requestId":nonce,"sessionId":state["sessionId"],
            "leafId":record["leafId"],"question":record["question"],"answer":record["answer"]});
        let id = self
            .transport
            .begin(
                "prompt",
                json!({"message":format!("/ait-btw-branch-v1 {arguments}")}),
            )
            .await?;
        Ok(self.actions.btw_branch(id, nonce))
    }
}
