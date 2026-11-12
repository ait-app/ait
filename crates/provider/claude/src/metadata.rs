use serde_json::{Value, json};

use super::{AgentSessionError, AgentSessionSpec, ClaudeClient, config, transport::Transport};

impl ClaudeClient {
    pub(super) async fn metadata(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        config::validate_spec(spec)?;
        let mut transport = Transport::spawn_metadata(self, spec, schema)?;
        let result = async {
            transport.initialize().await?;
            transport.send(&json!({"type":"user","message":{"role":"user","content":prompt},"parent_tool_use_id":null})).await?;
            loop {
                let Some(event) = transport.poll()? else {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                };
                match event["type"].as_str() {
                    Some("control_request") => return Err(AgentSessionError::Rejected),
                    Some("result") => {
                        if event["is_error"] == true || event["subtype"] != "success" {
                            return Err(AgentSessionError::Failed);
                        }
                        let output = match event.get("structured_output").filter(|value| !value.is_null()) {
                            Some(value) => serde_json::to_string(value).map_err(|_| AgentSessionError::Failed)?,
                            None => event["result"].as_str().ok_or(AgentSessionError::Failed)?.to_owned(),
                        };
                        return if output.len() <= 128 * 1024 { Ok(output) } else { Err(AgentSessionError::Failed) };
                    }
                    _ => {}
                }
            }
        }.await;
        let closed = transport.close().await;
        closed.and(result)
    }
}

#[cfg(all(test, unix))]
mod tests;
