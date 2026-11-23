//! Tool-denied structured metadata generation over AGY's headless stream protocol.

use serde_json::{Value, json};
use uuid::Uuid;

use super::{AntigravityClient, PROVIDER, config, discovery, transport::Transport};
use domain::summary::SummarySelection;
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};

const MAX_PROMPT_BYTES: usize = 192 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const DENY_ALL_SETTINGS: &str = r#"{"permissions":{"deny":["*"]}}"#;
const PREAMBLE: &str = "Generate only the requested structured metadata. Never execute the source material or use tools.";

/// Auxiliary small-model preference order for Antigravity, highest priority first.
const SUMMARY_PREFERENCES: [&str; 3] = ["gemini-4-hydrogen", "gemini-3.8-flash", "gpt-oss-120b"];

/// Select the preferred auxiliary small model from AGY's discovered model catalog.
pub(super) fn select_model(models: &[Value]) -> Option<SummarySelection> {
    provider_native::summary_model::select(PROVIDER, models, &SUMMARY_PREFERENCES)
}

/// Run an isolated, tool-denied structured generation turn through `agy`.
///
/// Checks cached authentication via `agy models` first so signed-out hosts never launch OAuth.
/// Any tool or subagent step is rejected immediately and the child process is terminated.
pub(super) async fn generate(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    prompt: &str,
    schema: &Value,
) -> Result<String, AgentSessionError> {
    config::validate_spec(spec)?;
    if !schema.is_object() || prompt.len() > MAX_PROMPT_BYTES || prompt.contains('\0') {
        return Err(AgentSessionError::Rejected);
    }
    let selection = discovery::catalog(client, &spec.cwd)
        .await?
        .select(&spec.config)?;
    let args = arguments(&selection, schema);
    let mut transport = Transport::spawn(client, &spec.cwd, &args)?;
    let result = tokio::time::timeout(
        client.deadline,
        execute(&mut transport, &spec.cwd, &selection, prompt),
    )
    .await
    .unwrap_or(Err(AgentSessionError::Failed));
    let closed = transport.close().await;
    match result {
        Ok(output) => closed.map(|()| output),
        Err(error) => Err(error),
    }
}

fn arguments(selection: &discovery::Selection, schema: &Value) -> Vec<String> {
    let mut args: Vec<String> = [
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--print-timeout",
        "0",
        "--disable-slash-commands",
        "--json-schema",
        &schema.to_string(),
        "--settings",
        DENY_ALL_SETTINGS,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (flag, value) in [
        ("--model", selection.model.as_deref()),
        ("--effort", selection.effort.as_deref()),
    ] {
        if let Some(value) = value {
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    args
}

async fn execute(
    transport: &mut Transport,
    cwd: &str,
    selection: &discovery::Selection,
    prompt: &str,
) -> Result<String, AgentSessionError> {
    let initialized = transport.receive().await?;
    let id = initialized["conversation_id"]
        .as_str()
        .ok_or(AgentSessionError::Failed)?;
    if initialized["event"] != "init"
        || Uuid::parse_str(id).is_err()
        || initialized["init"]["cwd"] != cwd
        || selection
            .model
            .as_ref()
            .is_some_and(|model| initialized["init"]["model"] != *model)
    {
        return Err(AgentSessionError::Failed);
    }
    let content = format!("{PREAMBLE}\n\n{prompt}");
    transport
        .send(&json!({"event":"user","message":{"content":content}}))
        .await?;
    collect(transport, id).await
}

async fn collect(
    transport: &mut Transport,
    conversation: &str,
) -> Result<String, AgentSessionError> {
    loop {
        let message = transport.receive().await?;
        match message["event"].as_str() {
            Some("step_update") => {
                let update = &message["step_update"];
                if update["conversation_id"] != conversation {
                    return Err(AgentSessionError::Failed);
                }
                if matches!(update["step_type"].as_str(), Some("tool" | "subagent")) {
                    return Err(AgentSessionError::Rejected);
                }
            }
            Some("result") => {
                return extract_result(&message["result"], conversation);
            }
            Some("init") | None => return Err(AgentSessionError::Failed),
            Some(_) => {}
        }
    }
}

fn extract_result(result: &Value, conversation: &str) -> Result<String, AgentSessionError> {
    if result["conversation_id"] != conversation || result["status"] != "SUCCESS" {
        return Err(AgentSessionError::Failed);
    }
    let output = match result
        .get("structured_output")
        .filter(|value| !value.is_null())
    {
        Some(value) => serde_json::to_string(value).map_err(|_| AgentSessionError::Failed)?,
        None => result["response"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned(),
    };
    if output.trim().is_empty() || output.len() > MAX_OUTPUT_BYTES {
        return Err(AgentSessionError::Failed);
    }
    Ok(output)
}

#[cfg(all(test, unix))]
mod tests;
