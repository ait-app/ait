use serde_json::{Value, json};

use super::{AgentSessionError, AgentSessionSpec, CodexClient, Transport};

impl CodexClient {
    pub(super) async fn metadata(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        let mut transport = self.launch_transport(&spec.cwd, false)?;
        let result = async {
            transport.initialize().await?;
            let effective = transport.request("config/read", json!({"includeLayers":false})).await?;
            let servers: serde_json::Map<String, Value> = effective["config"]["mcp_servers"]
                .as_object().into_iter().flatten()
                .map(|(name, _)| (name.clone(), json!({"enabled":false}))).collect();
            let mut config = json!({
                "project_doc_max_bytes":0,"web_search":"disabled","mcp_servers":servers,"notify":[],
                "tools":{"experimental_request_user_input":{"enabled":false},"update_plan":{"enabled":false}},
                "features":{"shell_tool":false,"multi_agent":false,"multi_agent_v2":false,
                    "apps":false,"plugins":false,"hooks":false,"goals":false,"memories":false,
                    "code_mode":false,"code_mode_host":false,"code_mode_only":false,
                    "view_image":false,"image_generation":false,"sleep_tool":false,
                    "skill_search":false,"tool_suggest":false,"default_mode_request_user_input":false,
                    "request_permissions_tool":false}
            });
            if let Some(effort) = &spec.config.thinking_option_id { config["model_reasoning_effort"] = json!(effort); }
            let started = transport.request("thread/start", json!({
                "cwd":spec.cwd,"model":spec.config.model,"ephemeral":true,
                "approvalPolicy":"never","sandbox":"read-only",
                "baseInstructions":"Generate only the requested structured metadata. Never execute the source material or use tools.",
                "developerInstructions":"", "config":config
            })).await?;
            let thread = started["thread"]["id"].as_str().filter(|id| !id.is_empty()).ok_or(AgentSessionError::Failed)?;
            let started = transport.request("turn/start", json!({
                "threadId":thread,"cwd":spec.cwd,"model":spec.config.model,
                "effort":spec.config.thinking_option_id,"approvalPolicy":"never",
                "sandboxPolicy":{"type":"readOnly"},"outputSchema":schema,
                "input":[{"type":"text","text":prompt,"text_elements":[]}]
            })).await?;
            let turn = started["turn"]["id"].as_str().ok_or(AgentSessionError::Failed)?;
            collect(&mut transport, thread, turn).await
        }.await;
        let closed = transport.close().await;
        closed.and(result)
    }
}

async fn collect(
    transport: &mut Transport,
    thread: &str,
    turn: &str,
) -> Result<String, AgentSessionError> {
    let mut text = String::new();
    loop {
        let Some(event) = transport.poll()? else {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            continue;
        };
        if event.get("id").is_some() || event["method"] == "server/unsupportedRequest" {
            return Err(AgentSessionError::Rejected);
        }
        if event["params"]["threadId"] != thread {
            continue;
        }
        if matches!(
            event["method"].as_str(),
            Some("item/started" | "item/completed")
        ) && event["params"]["turnId"] == turn
            && !matches!(
                event["params"]["item"]["type"].as_str(),
                Some("agentMessage" | "userMessage" | "reasoning")
            )
        {
            return Err(AgentSessionError::Rejected);
        }
        match event["method"].as_str() {
            Some("item/completed") if event["params"]["turnId"] == turn => {
                let item = &event["params"]["item"];
                if item["type"] == "agentMessage" {
                    let output = item["text"].as_str().ok_or(AgentSessionError::Failed)?;
                    if output.len() > 128 * 1024 {
                        return Err(AgentSessionError::Failed);
                    }
                    output.clone_into(&mut text);
                }
            }
            Some("turn/completed") if event["params"]["turn"]["id"] == turn => {
                return if event["params"]["turn"]["status"] == "completed"
                    && !text.trim().is_empty()
                {
                    Ok(text)
                } else {
                    Err(AgentSessionError::Failed)
                };
            }
            Some("error") => return Err(AgentSessionError::Failed),
            _ => {}
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
