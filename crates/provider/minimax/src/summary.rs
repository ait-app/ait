//! Isolated one-shot structured metadata generation via `mcode exec`.

use std::process::Stdio;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use super::{MiniMaxClient, PROVIDER, catalog, config};
use domain::summary::SummarySelection;
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider_native::process_group::{self, Signal};

const MAX_PROMPT_BYTES: usize = 192 * 1024;
const MAX_STDOUT_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const SYSTEM_PROMPT: &str = "Generate only the requested structured metadata. Never execute the source material or use tools.";

/// Preferred auxiliary small models for `MiniMax` Code, highest priority first.
const SUMMARY_PREFERENCES: [&str; 4] = ["minimax-m3.1-flash", "m3.1-flash", "m3.1 flash", "flash"];

/// Select `MiniMax` Code's auxiliary small model (defaulting to M3.1 Flash) and its lowest effort.
pub(super) fn select_model(models: &[Value]) -> Option<SummarySelection> {
    let mut selection =
        provider_native::summary_model::select(PROVIDER, models, &SUMMARY_PREFERENCES)?;
    if selection.thinking_option_id.is_none() {
        selection.thinking_option_id = models
            .iter()
            .find(|model| model["id"].as_str() == selection.model.as_deref())
            .and_then(|model| model["thinkingOptions"].as_array())
            .and_then(|options| {
                ["off", "thinking:minimal", "thinking:low"]
                    .into_iter()
                    .find(|effort| options.iter().any(|option| option["id"] == *effort))
            })
            .map(str::to_owned);
    }
    Some(selection)
}

/// Run an isolated, tool-off, single-step structured request via `mcode exec`.
pub(super) async fn generate(
    client: &MiniMaxClient,
    spec: &AgentSessionSpec,
    prompt: &str,
    schema: &Value,
) -> Result<String, AgentSessionError> {
    config::validate_spec(spec)?;
    if !schema.is_object() || prompt.len() > MAX_PROMPT_BYTES || prompt.contains('\0') {
        return Err(AgentSessionError::Rejected);
    }
    let (model, effort) = exec_model_args(
        spec.config.model.as_deref(),
        spec.config.thinking_option_id.as_deref(),
    )?;
    let mut command = Command::new(&client.program);
    command
        .args([
            "exec",
            "--input",
            "-",
            "--cwd",
            &spec.cwd,
            "--permission",
            "off",
            "--max-steps",
            "1",
            "--mode",
            "lightweight",
            "--output-format",
            "json",
            "--output-schema",
            &schema.to_string(),
            "--system-prompt",
            SYSTEM_PROMPT,
        ])
        .current_dir(&spec.cwd)
        .envs(client.environment.entries());
    if let Some(model) = model {
        command.args(["--model", &model]);
    }
    if let Some(effort) = effort {
        command.args(["--effort", &effort]);
    }
    let stdout = tokio::time::timeout(client.deadline, run_with_input(&mut command, prompt))
        .await
        .map_err(|_| AgentSessionError::Failed)??;
    parse_output(&stdout)
}

/// Map AIT's `MiniMax` model and thinking IDs into `mcode exec`'s `--model` and `--effort` flags.
fn exec_model_args(
    model: Option<&str>,
    thinking: Option<&str>,
) -> Result<(Option<String>, Option<String>), AgentSessionError> {
    let Some(raw_model) = model else {
        let effort = bare_effort(thinking);
        return Ok((None, effort));
    };
    let (base, inline_variant) = catalog::split(raw_model);
    let formatted = format_exec_model(base)?;
    let (variant, effort) = match thinking {
        None => (inline_variant.filter(|v| !v.is_empty()), None),
        Some("off") => (None, None),
        Some("thinking") => (Some("thinking"), None),
        Some(value) => match value.rsplit_once(':') {
            Some((variant, level)) if !variant.is_empty() && !level.is_empty() => (
                (variant != "off").then_some(variant),
                (level != "default").then_some(level),
            ),
            _ => (
                inline_variant.filter(|v| !v.is_empty()),
                (value != "default").then_some(value),
            ),
        },
    };
    let model = match variant {
        Some(variant) => format!("{formatted}#{variant}"),
        None => formatted,
    };
    Ok((Some(model), effort.map(str::to_owned)))
}

fn bare_effort(thinking: Option<&str>) -> Option<String> {
    let value = thinking?;
    let level = value.rsplit_once(':').map_or(value, |(_, level)| level);
    (!matches!(level, "" | "off" | "thinking" | "default")).then(|| level.to_owned())
}

fn format_exec_model(base: &str) -> Result<String, AgentSessionError> {
    if let Some(rest) = base.strip_prefix("m:") {
        let (provider, name) = rest.split_once(':').ok_or(AgentSessionError::Rejected)?;
        if provider.is_empty() || name.is_empty() {
            return Err(AgentSessionError::Rejected);
        }
        return Ok(format!("{provider}/{name}"));
    }
    let (provider, name) = base.split_once('/').ok_or(AgentSessionError::Rejected)?;
    if provider.is_empty() || name.is_empty() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(base.to_owned())
}

fn parse_output(stdout: &str) -> Result<String, AgentSessionError> {
    let response: Value = serde_json::from_str(stdout).map_err(|_| AgentSessionError::Failed)?;
    if response["status"] != "succeeded" {
        return Err(AgentSessionError::Failed);
    }
    let output = match response.get("output") {
        Some(Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => {
            serde_json::to_string(value).map_err(|_| AgentSessionError::Failed)?
        }
        _ => return Err(AgentSessionError::Failed),
    };
    if output.trim().is_empty() || output.len() > MAX_OUTPUT_BYTES {
        return Err(AgentSessionError::Failed);
    }
    Ok(output)
}

/// One-shot child whose Unix process group is killed on drop (including timeout cancellation).
struct ExecProcess {
    child: Child,
    group: Option<u32>,
}

impl Drop for ExecProcess {
    fn drop(&mut self) {
        process_group::signal(self.group, Signal::Kill);
        let _ = self.child.start_kill();
    }
}

async fn run_with_input(command: &mut Command, prompt: &str) -> Result<String, AgentSessionError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let child = command
        .spawn()
        .map_err(|_| AgentSessionError::Unavailable)?;
    let mut process = ExecProcess {
        group: child.id(),
        child,
    };
    let mut stdin = process
        .child
        .stdin
        .take()
        .ok_or(AgentSessionError::Failed)?;
    stdin
        .write_all(prompt.as_bytes())
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    stdin
        .shutdown()
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    drop(stdin);
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or(AgentSessionError::Failed)?;
    let mut bytes = Vec::new();
    stdout
        .take(MAX_STDOUT_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_STDOUT_BYTES {
        return Err(AgentSessionError::Failed);
    }
    let status = process
        .child
        .wait()
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if !status.success() {
        return Err(AgentSessionError::Failed);
    }
    String::from_utf8(bytes).map_err(|_| AgentSessionError::Failed)
}

#[cfg(all(test, unix))]
mod tests;
