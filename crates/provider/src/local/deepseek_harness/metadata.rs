//! Native one-shot profile, retaining model adapters and credentials but no tools or journal.
use std::collections::BTreeSet;

use serde_json::{Value, json};
use tokio::process::Command;

use super::DeepSeekHarnessClient;
use crate::local::metadata_process;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

impl DeepSeekHarnessClient {
    pub(super) async fn metadata(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        let model: Vec<String> = serde_json::from_str(
            spec.config
                .model
                .as_deref()
                .ok_or(AgentSessionError::Rejected)?,
        )
        .map_err(|_| AgentSessionError::Rejected)?;
        if model.len() != 2 || model.iter().any(|part| part.trim().is_empty()) {
            return Err(AgentSessionError::Rejected);
        }
        let mut probe = self.metadata_command(&spec.cwd);
        probe.args(["--profile", "headless", "--dump-config"]);
        let configuration = metadata_process::output(&mut probe).await?;
        let mut patch = isolated_patch(&configuration, &model, prompt, schema)?;
        if let Some(effort) = &spec.config.thinking_option_id {
            if model[0] != "deepseek-official" {
                return Err(AgentSessionError::Rejected);
            }
            patch
                .as_array_mut()
                .expect("isolated patch is an array")
                .push(json!({"id":"llm-deepseek","config":{"reasoningEffort":effort}}));
        }
        let directory = tempfile::tempdir().map_err(|_| AgentSessionError::Failed)?;
        let patch_path = directory.path().join("metadata.json");
        tokio::fs::write(&patch_path, patch.to_string())
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        let mut command = self.metadata_command(&spec.cwd);
        command
            .args(["--profile", "headless", "--patch"])
            .arg(&patch_path);
        metadata_process::output(&mut command).await
    }

    fn metadata_command(&self, cwd: &str) -> Command {
        let mut command = Command::new(&self.program);
        command.current_dir(cwd).envs(self.environment.entries());
        command
    }
}

fn isolated_patch(
    configuration: &str,
    model: &[String],
    prompt: &str,
    schema: &Value,
) -> Result<Value, AgentSessionError> {
    let mut ids = BTreeSet::new();
    let mut patch = Vec::new();
    for line in configuration
        .lines()
        .filter_map(|line| line.strip_prefix("- id: "))
    {
        let id = line.trim().trim_matches(['\'', '"']);
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !ids.insert(id)
        {
            return Err(AgentSessionError::Failed);
        }
        // Disable every composed root first, including user tool/persistence plugins.
        patch.push(json!({"id":id,"disabled":true}));
    }
    let core = [
        "timer",
        "llm",
        "deepseek-llm-api-extensions",
        "session",
        "session-log-deepseek",
        "session-title",
        "agent",
        "agent-default-model",
        "llm-retry",
        "credentials",
        "llm-pi-ai",
        "llm-deepseek",
        "session-projection",
        "tools",
        "system-prompt",
        "agent-loop",
        "headless-runner",
    ];
    if !core.iter().all(|id| ids.contains(id)) {
        return Err(AgentSessionError::Unavailable);
    }
    patch.extend(core.iter().map(|id| json!({"id":id,"disabled":false})));
    patch.extend([
        json!({"id":"headless-runner","inject":[],"config":{"task":format!("{prompt}\nReturn only JSON matching this schema: {schema}")}}),
        json!({"id":"agent-default-model","config":{"provider":model[0],"model":model[1]}}),
        json!({"id":"tools","config":{"mode":"native"}}),
        json!({"id":"agent-loop","config":{"agents":[]}}),
        json!({"id":"system-prompt","config":{"includeHarnessIdentity":false,"includeRuntimeContext":false,
            "personaPrefix":"Generate only the requested metadata from the supplied text.","personaSuffix":""}}),
    ]);
    Ok(Value::Array(patch))
}

#[cfg(test)]
mod tests;
