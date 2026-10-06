//! Completion waits observe committed state rather than consuming session commands.

use super::{AgentExecution, Duration, ErrorCode, Value, json};
use crate::protocol::agent_execution::WaitRequest;
use crate::rpc::agent_execution::only;

impl AgentExecution {
    pub(super) async fn wait(&self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId", "timeoutMs"])?;
        let request: WaitRequest =
            serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
        if request
            .timeout_ms
            .is_some_and(|timeout| timeout == 0 || timeout > 9_007_199_254_740_991)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let deadline = request
            .timeout_ms
            .map(|timeout| {
                tokio::time::Instant::now()
                    .checked_add(Duration::from_millis(timeout))
                    .ok_or(ErrorCode::InvalidMessage)
            })
            .transpose()?;
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::AgentIo);
        }
        let template = self.0.template.clone();
        let observed = tokio::task::spawn_blocking(move || {
            let state = template.lock().map_err(|_| ErrorCode::AgentIo)?.fork(None);
            let id = state.resolve(&request.agent_id)?;
            state.owners.observe(&id, state.wait_result(&id)?)
        })
        .await
        .map_err(|_| ErrorCode::AgentIo)?;
        let mut observed = match observed {
            Ok(observed) => observed,
            Err(ErrorCode::AgentNotFound) => {
                return Ok(json!({"status":"error","final":null,
                "error":"Agent not found","lastMessage":null}));
            }
            Err(error) => return Err(error),
        };
        let retain_text = observed.borrow()["live"] == true;
        loop {
            let result = observed.borrow_and_update().clone();
            if result["status"] != "running" && result["busy"] != true {
                return Ok(finish(result, retain_text));
            }
            let expire = async {
                if let Some(deadline) = deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                () = self.0.cancellation.cancelled() => return Err(ErrorCode::AgentIo),
                () = expire => {
                    let mut result = finish(observed.borrow().clone(), false);
                    result["status"] = json!("timeout");
                    result["lastMessage"] = Value::Null;
                    result["error"] = Value::Null;
                    return Ok(result);
                }
                result = observed.changed() => result.map_err(|_| ErrorCode::AgentIo)?,
            }
        }
    }
}

fn finish(mut result: Value, retain_text: bool) -> Value {
    if retain_text && result["lastMessage"].is_null() && result["final"]["archivedAt"].is_string() {
        result["lastMessage"] = result["completionText"].clone();
    }
    if let Some(object) = result.as_object_mut() {
        object.remove("completionText");
        object.remove("live");
        object.remove("busy");
    }
    result
}
