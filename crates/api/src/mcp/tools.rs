//! Translate the agent-facing Paseo tool vocabulary to existing Ait operations.

use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::catalog::Tool;

mod creation;
mod queries;
mod schedules;

/// Execute `tool` with validated `arguments` using `state` and optional caller inheritance.
/// # Errors
/// Returns invalid placement, unavailable services or existing capability RPC failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &Tool,
    mut arguments: Value,
    caller_id: Option<&str>,
) -> Result<Value, ErrorCode> {
    let caller = if let Some(id) = caller_id {
        let value = rpc::execute(state, "agent.get.request", json!({"agentId":id})).await?;
        if !value["agent"].is_object() {
            return Err(ErrorCode::InvalidMessage);
        }
        Some(value["agent"].clone())
    } else {
        None
    };
    match tool.name.as_str() {
        "create_agent" => creation::agent(state, arguments, caller.as_ref()).await,
        "create_workspace" => {
            let params = creation::workspace(arguments, caller.as_ref())?;
            rpc::execute(state, "workspace.create.request", params).await
        }
        "create_terminal" => {
            creation::directory(state, &mut arguments, caller.as_ref()).await?;
            rpc::execute(state, "terminal.create.request", arguments).await
        }
        "send_agent_prompt" => {
            rename(&mut arguments, "prompt", "text")?;
            rpc::execute(state, "agent.message.send.request", arguments).await
        }
        "get_agent_status" => queries::status(state, arguments).await,
        "list_agents" => queries::agents(state, &arguments).await,
        "list_workspaces" => queries::workspaces(state, &arguments).await,
        "list_pending_permissions" => queries::permissions(state).await,
        "list_profiles" => queries::profiles(state).await,
        "get_agent_activity" => queries::activity(state, arguments).await,
        "inspect_provider" => queries::provider(state, arguments, caller.as_ref()).await,
        "update_agent" => update_agent(state, arguments).await,
        "kill_agent" => {
            let mut result = rpc::execute(
                state,
                "agent.items.close.request",
                json!({"agentIds":[arguments["agentId"]]}),
            )
            .await?;
            result["success"] = json!(result["agents"][0]["agentId"] == arguments["agentId"]);
            Ok(result)
        }
        "wait_for_agent" => {
            if arguments.get("timeoutMs").is_none() {
                arguments["timeoutMs"] = json!(30_000);
            }
            rpc::execute(state, "agent.finish.wait.request", arguments).await
        }
        "rename_workspace" => {
            if arguments.get("workspaceId").is_none() {
                arguments["workspaceId"] = caller
                    .as_ref()
                    .and_then(|caller| caller.get("workspaceId"))
                    .cloned()
                    .ok_or(ErrorCode::InvalidMessage)?;
            }
            rpc::execute(state, "workspace.title.set.request", arguments).await
        }
        "capture_terminal" => {
            if take(&mut arguments, "scrollback")? == Some(json!(true)) {
                arguments["start"] = json!(0);
            }
            rpc::execute(state, "terminal.capture.request", arguments).await
        }
        "send_terminal_keys" => terminal_keys(state, &arguments).await,
        "create_schedule" | "create_heartbeat" => {
            schedules::create(
                state,
                arguments,
                caller.as_ref(),
                tool.name == "create_heartbeat",
            )
            .await
        }
        "list_schedules" => schedules::list(state).await,
        "inspect_schedule" | "pause_schedule" | "resume_schedule" | "delete_schedule"
        | "schedule_logs" | "run_schedule_once" | "update_schedule" | "delete_heartbeat" => {
            schedules::operate(state, tool, arguments, caller.as_ref()).await
        }
        _ => rpc::execute(state, &tool.methods[0], arguments).await,
    }
}

async fn update_agent(state: &Shared, mut arguments: Value) -> Result<Value, ErrorCode> {
    let settings = take(&mut arguments, "settings")?;
    let id = arguments["agentId"].clone();
    // Validate and apply provider settings before committing the unrelated display metadata.
    let settings_result = if let Some(mut settings) = settings {
        rename(&mut settings, "model", "modelId")?;
        rename(&mut settings, "features", "featureValues")?;
        Some(
            rpc::execute(
                state,
                "agent.config.apply.request",
                json!({"agentId":id,"config":settings}),
            )
            .await?,
        )
    } else {
        None
    };
    if settings_result.as_ref().is_some_and(queries::failed) {
        return Ok(settings_result.expect("failed settings result exists"));
    }
    if arguments.get("name").is_none() && arguments.get("labels").is_none() {
        return settings_result.ok_or(ErrorCode::InvalidMessage);
    }
    rpc::execute(state, "agent.update.request", arguments).await
}

async fn terminal_keys(state: &Shared, arguments: &Value) -> Result<Value, ErrorCode> {
    let keys = arguments["keys"]
        .as_str()
        .ok_or(ErrorCode::InvalidMessage)?;
    let data = if arguments["literal"] == true {
        keys
    } else {
        key_token(keys)
    };
    terminal::connection::TerminalConnection::default()
        .event(
            json!({"terminalId":arguments["terminalId"],"message":{"type":"input","data":data}}),
            &state.terminal,
        )
        .await?;
    Ok(json!({"success":true}))
}

fn key_token(keys: &str) -> &str {
    match keys {
        "Enter" => "\r",
        "Tab" => "\t",
        "Escape" => "\u{1b}",
        "Space" => " ",
        "BSpace" => "\u{7f}",
        "C-c" => "\u{3}",
        "C-d" => "\u{4}",
        "C-z" => "\u{1a}",
        "C-l" => "\u{c}",
        "C-a" => "\u{1}",
        "C-e" => "\u{5}",
        _ => keys,
    }
}

fn take(arguments: &mut Value, key: &str) -> Result<Option<Value>, ErrorCode> {
    Ok(arguments
        .as_object_mut()
        .ok_or(ErrorCode::InvalidMessage)?
        .remove(key))
}

fn rename(arguments: &mut Value, old: &str, new: &str) -> Result<(), ErrorCode> {
    if let Some(value) = take(arguments, old)? {
        arguments[new] = value;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
