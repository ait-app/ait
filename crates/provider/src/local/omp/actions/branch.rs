//! Correlate the public BTW extension's acknowledgement and final command completion.

use super::{Kind, Pending};
use crate::ports::agent_session::AgentSessionFuture;
use serde_json::{Value, json};

impl Pending {
    /// Fence history until both the bridge acknowledgement and native command completion arrive.
    pub(in crate::local::omp) fn btw_branch(
        &mut self,
        id: String,
        nonce: String,
    ) -> AgentSessionFuture<'static, Value> {
        self.track(
            id,
            "btw_branch",
            Kind::BtwBranch {
                nonce,
                receipt: None,
            },
        )
    }

    /// Consume only the owned bridge acknowledgement and its prompt receipts.
    pub(super) fn capture_btw_branch(&mut self, event: &Value) -> Option<bool> {
        if event["type"] == "extension_ui_request" && event["method"] == "notify" {
            let text = event["message"]
                .as_str()?
                .strip_prefix("AIT_BTW_BRANCH_V1:")?;
            let value: Value = serde_json::from_str(text).ok()?;
            for request in self.requests.values_mut() {
                if let Kind::BtwBranch { nonce, receipt } = &mut request.kind
                    && value["requestId"] == nonce.as_str()
                {
                    *receipt = Some(value);
                    return Some(true);
                }
            }
            return Some(true);
        }
        let id = event["id"].as_str()?;
        let request = self.requests.get(id)?;
        let Kind::BtwBranch { receipt, .. } = &request.kind else {
            return None;
        };
        let completed = (event["type"] == "prompt_result")
            || (event["type"] == "response"
                && (event["success"] != true || event["data"]["agentInvoked"].is_boolean()));
        if !completed {
            return Some(event["type"] == "response");
        }
        let accepted = if event["type"] == "prompt_result" {
            event["status"] == "completed" && event["agentInvoked"] == false
        } else {
            event["success"] == true && event["data"]["agentInvoked"] == false
        };
        let success = accepted
            && receipt
                .as_ref()
                .is_some_and(|value| value["success"] == true);
        let error = receipt
            .as_ref()
            .and_then(|value| value["error"].as_str())
            .unwrap_or("OMP did not acknowledge the BTW branch");
        let result = json!({"type":"response","id":id,"command":"btw_branch",
            "success":success,"data":{},"error":error});
        self.requests
            .get_mut(id)
            .expect("owned bridge request")
            .kind = Kind::Transient;
        Some(self.capture(&result))
    }
}

#[cfg(test)]
mod tests;
