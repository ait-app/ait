//! Translate the agent-facing Paseo tool vocabulary to existing Ait operations.

use std::time::Instant;

use bot::mcp::CallerKind;
use bot::recall::Sight;
use chrono::{DateTime, Utc};
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::Scope;
use super::catalog::Tool;
use super::scope::{self, Failure};

pub(super) mod creation;
mod notepad;
mod overview;
mod queries;
pub(super) mod recall;
mod schedules;
mod tickets;

/// Execute `tool` with validated `arguments` using `state`, confining them to `scope` for scoped
/// credentials and inheriting placement and parentage from the resolved `caller` Agent.
/// # Errors
/// Returns scope violations and guided scope refusals, reserved labels, invalid placement,
/// unavailable services or existing capability RPC failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &Tool,
    mut arguments: Value,
    scope: Option<&Scope>,
    caller: Option<&Value>,
) -> Result<Value, Failure> {
    // Taken before anything awaits, so an observation belongs to the Bot turn that made the call.
    let started = Instant::now();
    // Schedules and heartbeats are one kind of record below this layer.
    if tool.name == "heartbeat_delete" {
        rename(&mut arguments, "heartbeatId", "scheduleId")?;
    }
    match scope {
        Some(scope) => scope::confine(state, scope, &tool.name, &mut arguments, caller).await?,
        // Host callers may not write `ait.parent.` labels either; that check is in `mcp::call`,
        // since calls from sibling Hosts also run unscoped here, carrying the parent labels
        // `mcp::peer` wrote.
        None if matches!(tool.name.as_str(), "agent_create" | "agent_update") => {
            scope::reserved_labels(&arguments, domain::bot::BOT_LABEL_PREFIX)?;
        }
        None => {}
    }
    if overview::handles(&tool.name) {
        return overview::execute(state, &tool.name, arguments, scope).await;
    }
    if recall::handles(&tool.name) {
        return recall::execute(state, &tool.name, &arguments, scope).await;
    }
    if tickets::handles(&tool.name) {
        return tickets::execute(state, &tool.name, arguments, scope, caller).await;
    }
    if let Some(scope) = scope
        && tool.name == "agent_create"
    {
        return tickets::create_agent(state, scope, arguments, caller).await;
    }
    let ticket_note = match scope {
        Some(scope) if tool.name == "agent_prompt" => {
            tickets::reopen_for_prompt(state, scope, &arguments).await
        }
        _ => None,
    };
    let mut result = dispatch(state, tool, arguments, scope, caller).await?;
    if let Some(note) = ticket_note
        && let Some(result) = result.as_object_mut()
    {
        result.insert("ticketNote".to_owned(), Value::String(note));
    }
    if let Some(scope) = scope {
        observe(state, scope, &tool.name, &result, started).await;
        scope::filter(scope, &tool.name, &mut result);
    }
    Ok(result)
}

/// Agent statuses a read reports once the Agent no longer runs a turn.
const SETTLED: [&str; 3] = ["idle", "error", "permission"];

/// Tell the Bot service that a Bot conversation's turn read an Agent settled through a call that
/// began at `started`, so the notification about what it read does not wake that conversation
/// again (ADR-271).
async fn observe(state: &Shared, scope: &Scope, tool: &str, result: &Value, started: Instant) {
    let CallerKind::Bot {
        conversation_id, ..
    } = scope.caller.kind
    else {
        return;
    };
    let (Some(bots), Some((agent, at))) = (&state.bot.bots, settled(tool, result)) else {
        return;
    };
    if let Err(error) = bots.observe(conversation_id, agent, at, started).await {
        tracing::debug!(?error, "Bot observation was not recorded");
    }
}

/// The Agent a successful `tool` `result` shows no longer running, with the latest time the
/// read reflects: when the Agent last changed, or when it raised the attention its snapshot
/// shows if later, since a permission request raises attention without changing the Agent.
fn settled(tool: &str, result: &Value) -> Option<(String, DateTime<Utc>)> {
    let snapshot = match tool {
        "agent_wait" => &result["final"],
        "agent_get" => &result["snapshot"],
        "agent_activity_read" => result,
        _ => return None,
    };
    if !SETTLED.contains(&result["status"].as_str()?) {
        return None;
    }
    let agent = snapshot["id"]
        .as_str()
        .or_else(|| snapshot["agentId"].as_str())?;
    let updated = timestamp(&snapshot["updatedAt"])?;
    let at =
        timestamp(&snapshot["attentionTimestamp"]).map_or(updated, |raised| raised.max(updated));
    Some((agent.to_owned(), at))
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let at = DateTime::parse_from_rfc3339(value.as_str()?).ok()?;
    Some(at.with_timezone(&Utc))
}

async fn dispatch(
    state: &Shared,
    tool: &Tool,
    mut arguments: Value,
    scope: Option<&Scope>,
    caller: Option<&Value>,
) -> Result<Value, ErrorCode> {
    let workspaces = scope.map(Scope::workspaces);
    match tool.name.as_str() {
        "agent_create" => creation::agent(state, arguments, caller).await,
        "workspace_create" => {
            let params = creation::workspace(arguments, caller)?;
            rpc::execute(state, "workspace.create.request", params).await
        }
        "terminal_create" => {
            creation::directory(state, &mut arguments, caller).await?;
            rpc::execute(state, "terminal.create.request", arguments).await
        }
        "agent_prompt" => {
            rename(&mut arguments, "prompt", "text")?;
            rpc::execute(state, "agent.message.send.request", arguments).await
        }
        "agent_get" => queries::status(state, arguments).await,
        "agent_list" => {
            let sight = reach(state, scope, caller).await?;
            queries::agents(state, &arguments, workspaces, sight.as_ref()).await
        }
        "workspace_list" => queries::workspaces(state, &arguments).await,
        "permission_list" => {
            let sight = reach(state, scope, caller).await?;
            queries::permissions(state, workspaces, sight.as_ref()).await
        }
        "profile_list" => queries::profiles(state).await,
        "agent_activity_read" => queries::activity(state, arguments).await,
        "provider_get" => queries::provider(state, arguments, caller).await,
        "agent_update" => update_agent(state, arguments).await,
        "agent_wait" => {
            if arguments.get("timeoutMs").is_none() {
                arguments["timeoutMs"] = json!(30_000);
            }
            rpc::execute(state, "agent.finish.wait.request", arguments).await
        }
        "workspace_rename" => {
            if arguments.get("workspaceId").is_none() {
                arguments["workspaceId"] = caller
                    .and_then(|caller| caller.get("workspaceId"))
                    .cloned()
                    .ok_or(ErrorCode::InvalidMessage)?;
            }
            rpc::execute(state, "workspace.title.set.request", arguments).await
        }
        "terminal_capture" => {
            if take(&mut arguments, "scrollback")? == Some(json!(true)) {
                arguments["start"] = json!(0);
            }
            rpc::execute(state, "terminal.capture.request", arguments).await
        }
        "terminal_send" => terminal_keys(state, &arguments).await,
        "schedule_create" | "heartbeat_create" => {
            schedules::create(state, arguments, caller, tool.name == "heartbeat_create").await
        }
        "schedule_list" => schedules::list(state).await,
        "heartbeat_list" => schedules::heartbeats(state, caller).await,
        "notepad_read" | "notepad_write" => {
            notepad::execute(state, tool, arguments, scope, caller).await
        }
        "schedule_get" | "schedule_pause" | "schedule_resume" | "schedule_delete"
        | "schedule_log_read" | "schedule_run" | "schedule_update" | "heartbeat_delete" => {
            schedules::operate(state, tool, arguments, caller).await
        }
        _ => rpc::execute(state, &tool.methods[0], arguments).await,
    }
}

/// Which Agents a scoped caller may list (ADR-280 §4), with `caller` a subtree caller's own
/// Agent record; `None` lists every Agent the Workspace scope allows.
async fn reach(
    state: &Shared,
    scope: Option<&Scope>,
    caller: Option<&Value>,
) -> Result<Option<Sight>, ErrorCode> {
    match scope {
        Some(scope) => recall::sight(state, scope, caller).await,
        None => Ok(None),
    }
}

async fn update_agent(state: &Shared, mut arguments: Value) -> Result<Value, ErrorCode> {
    let settings = take(&mut arguments, "settings")?;
    rename(&mut arguments, "title", "name")?;
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
