//! Authorization and argument rewriting for Bot-scoped MCP callers.
//!
//! A scoped caller may reach only its grant's Workspaces: the root and the unarchived worktrees
//! its Bot created. Every target id is resolved through the existing capability RPCs and replaced
//! by the canonical id, so later RPCs never re-resolve a prefix or title to another target.

use std::collections::BTreeSet;

use bot::mcp::{Caller, CallerKind, Grant};
use domain::bot::permissions::BotPermissions;
use domain::bot::{BOT_LABEL_PREFIX, BOT_SCOPE_LABEL, BOT_WORKSPACES};
use model::ErrorCode;
use serde_json::{Map, Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::tools::creation;

/// The only native feature a scoped caller may set; others can change execution permissions.
const ALLOWED_FEATURE: &str = "fast_mode";

/// A scoped credential's identity with the grant read once for the current request.
#[derive(Debug)]
pub(super) struct Scope {
    /// Bot session or subtree Agent that presented the credential.
    pub(super) caller: Caller,
    /// Current tool groups, mode cap and Workspaces of the caller's root Workspace.
    pub(super) grant: Grant,
}

impl Scope {
    /// Whether `tool` is granted; Bot-only tools are granted to Bot sessions, never subtrees.
    pub(super) fn allows(&self, tool: &str) -> bool {
        let bot = matches!(self.caller.kind, CallerKind::Bot { .. });
        self.grant.permissions.allows(tool, bot)
    }

    /// The Workspaces this caller may reach, for filtering listings at their source.
    pub(super) fn workspaces(&self) -> &BTreeSet<String> {
        &self.grant.workspaces
    }

    /// Require `workspace` to be one of the grant's Workspaces.
    /// # Errors
    /// Returns `Forbidden` for a missing or foreign Workspace.
    fn contains(&self, workspace: Option<&str>) -> Result<(), ErrorCode> {
        if workspace.is_some_and(|workspace| self.grant.workspaces.contains(workspace)) {
            Ok(())
        } else {
            Err(ErrorCode::Forbidden)
        }
    }
}

/// How `confine` treats a tool; a tool without a rule is never executed for scoped callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// No Workspace or target arguments; whatever is implied comes from the credential.
    Open,
    /// Agent listings filtered at their source, or schedule listings filtered by `filter`.
    Listing,
    /// Provider discovery run in the root Workspace directory.
    RootDirectory,
    /// Agent creation placed, labelled and capped in scope.
    CreateAgent,
    /// One Agent target in scope.
    Agent,
    /// Agent mode change within the cap of the target's provider.
    AgentMode,
    /// Agent settings and labels, with modes capped and reserved labels refused.
    UpdateAgent,
    /// A required Workspace in scope.
    Workspace,
    /// A Workspace in scope, defaulting to the root.
    RootWorkspace,
    /// One terminal target in a scoped Workspace.
    Terminal,
    /// One schedule target in a scoped Workspace.
    Schedule,
    /// A schedule run or resumed only while its current provider and mode are within the cap.
    ScheduleControl,
    /// A schedule control whose patched provider and mode must also stay within the cap.
    UpdateSchedule,
    /// New-Agent schedule in a scoped Workspace, at the provider's default mode, in the Bot scope.
    CreateSchedule,
    /// Heartbeat targeting a subtree caller in a scoped Workspace; Bot sessions cannot be targets.
    CreateHeartbeat,
    /// A worktree of the root project, recorded in the Bot's scope.
    CreateWorkspace,
}

/// Return the rule of `tool`; tools must be registered here explicitly to be usable.
fn rule(tool: &str) -> Option<Rule> {
    Some(match tool {
        "list_profiles" | "list_providers" | "bot_memory_read" | "bot_memory_update"
        | "workspace_overview" => Rule::Open,
        "list_agents" | "list_pending_permissions" | "list_schedules" => Rule::Listing,
        "list_models" | "inspect_provider" => Rule::RootDirectory,
        "create_agent" => Rule::CreateAgent,
        "send_agent_prompt" | "cancel_agent" | "archive_agent" | "kill_agent"
        | "get_agent_status" | "get_agent_activity" | "wait_for_agent" => Rule::Agent,
        "set_agent_mode" => Rule::AgentMode,
        "update_agent" => Rule::UpdateAgent,
        "list_workspace_scripts" | "start_workspace_script" | "stop_workspace_script" => {
            Rule::Workspace
        }
        "list_terminals" | "create_terminal" => Rule::RootWorkspace,
        "kill_terminal" | "capture_terminal" | "send_terminal_keys" => Rule::Terminal,
        "inspect_schedule" | "pause_schedule" | "delete_schedule" | "schedule_logs"
        | "delete_heartbeat" => Rule::Schedule,
        "run_schedule_once" | "resume_schedule" => Rule::ScheduleControl,
        "update_schedule" => Rule::UpdateSchedule,
        "create_schedule" => Rule::CreateSchedule,
        "create_heartbeat" => Rule::CreateHeartbeat,
        "create_workspace" => Rule::CreateWorkspace,
        _ => return None,
    })
}

/// Rewrite or reject scoped `arguments` for `tool` before execution.
///
/// `context` is the resolved caller Agent of a subtree credential; Bot sessions have none.
/// Targets are resolved through existing RPCs and replaced by their canonical ids; a target
/// Workspace must be in the grant and unarchived. Directories always come from the server.
/// Created schedules and heartbeats record the root as their Bot scope.
/// # Errors
/// Returns `Forbidden` for unlisted tools, `cwd`, foreign or archived Workspaces, reserved
/// labels, features or modes outside the cap (including a schedule's current mode), and Bot
/// heartbeats; `ResourceExhausted` when the Bot already owns its maximum of worktrees; and
/// propagates target lookup failures.
pub(super) async fn confine(
    state: &Shared,
    scope: &Scope,
    tool: &str,
    arguments: &mut Value,
    context: Option<&Value>,
) -> Result<(), ErrorCode> {
    let root = scope.caller.root.as_str();
    match admit(tool, arguments)? {
        Rule::Open | Rule::Listing => Ok(()),
        Rule::RootDirectory => {
            arguments["cwd"] = active(state, root).await?["workspaceDirectory"].take();
            Ok(())
        }
        Rule::CreateAgent => {
            let inherited = context.and_then(|agent| agent["workspaceId"].as_str());
            rewrite_create_agent(scope, inherited, arguments)?;
            active(state, &text(&arguments["workspaceId"])?)
                .await
                .map(drop)
        }
        rule @ (Rule::Agent | Rule::AgentMode | Rule::UpdateAgent) => {
            let agent = creation::require_success(state, &arguments["agentId"]).await?;
            let id = text(&agent["id"])?;
            retarget(
                scope,
                "agentId",
                agent["workspaceId"].as_str(),
                &id,
                arguments,
            )?;
            match rule {
                Rule::AgentMode => cap_agent_mode(scope, &agent, arguments)?,
                Rule::UpdateAgent => cap_update_agent(scope, &agent, arguments)?,
                _ => {}
            }
            active(state, &text(&agent["workspaceId"])?).await.map(drop)
        }
        rule @ (Rule::Workspace | Rule::RootWorkspace | Rule::CreateSchedule) => {
            if rule != Rule::Workspace && arguments.get("workspaceId").is_none() {
                arguments["workspaceId"] = json!(root);
            }
            let workspace = text(&arguments["workspaceId"])?;
            scope.contains(Some(&workspace))?;
            if rule == Rule::CreateSchedule {
                let inherited = context.map(|agent| &agent["provider"]);
                rewrite_create_schedule(scope, inherited, arguments)?;
            }
            active(state, &workspace).await.map(drop)
        }
        Rule::CreateHeartbeat => {
            let workspace = heartbeat_workspace(scope, context)?;
            arguments["workspaceId"] = json!(workspace);
            arguments["botScope"] = json!(root);
            active(state, workspace).await.map(drop)
        }
        Rule::Terminal => terminal(state, scope, arguments).await,
        rule @ (Rule::Schedule | Rule::ScheduleControl | Rule::UpdateSchedule) => {
            let schedule = schedule(state, scope, arguments).await?;
            if rule != Rule::Schedule {
                cap_schedule(&scope.grant.permissions, &schedule)?;
            }
            if rule == Rule::UpdateSchedule {
                cap_schedule_update(&scope.grant.permissions, &schedule, arguments)?;
            }
            Ok(())
        }
        Rule::CreateWorkspace => {
            let root = active(state, root).await?;
            rewrite_create_workspace(scope, &root, arguments)
        }
    }
}

/// Record a worktree created by a scoped `create_workspace` in the Bot's scope.
///
/// `project` is the root project `confine` placed the worktree in. Business failures without a
/// Workspace pass through unchanged.
/// # Errors
/// Returns `Forbidden` when the created Workspace is not in `project`, and Bot service or
/// recording failures, including `ResourceExhausted` once the Bot owns its maximum.
pub(super) async fn adopt(
    state: &Shared,
    scope: &Scope,
    project: Option<&Value>,
    created: &Value,
) -> Result<(), ErrorCode> {
    let Some(workspace) = created.pointer("/workspace/id").and_then(Value::as_str) else {
        return Ok(());
    };
    if project.is_none() || created.pointer("/workspace/projectId") != project {
        return Err(ErrorCode::Forbidden);
    }
    let bots = state
        .bot
        .bots
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    bots.record_workspace(&scope.caller.root, workspace).await
}

/// Remove out-of-scope entries from list results that cannot be filtered at the source.
pub(super) fn filter(scope: &Scope, tool: &str, result: &mut Value) {
    if tool != "list_schedules" {
        return;
    }
    if let Some(schedules) = result.get_mut("schedules").and_then(Value::as_array_mut) {
        schedules.retain(|schedule| scope.contains(schedule["workspaceId"].as_str()).is_ok());
    }
}

/// Refuse labels in the reserved `ait.bot.` namespace, which no MCP caller may write.
/// # Errors
/// Returns `Forbidden` when `arguments.labels` names a reserved label.
pub(super) fn reserved_labels(arguments: &Value) -> Result<(), ErrorCode> {
    let reserved = arguments
        .get("labels")
        .and_then(Value::as_object)
        .is_some_and(|labels| labels.keys().any(|key| key.starts_with(BOT_LABEL_PREFIX)));
    if reserved {
        Err(ErrorCode::Forbidden)
    } else {
        Ok(())
    }
}

/// Return the rule of a registered `tool`, refusing caller-supplied directories.
/// # Errors
/// Returns `Forbidden` for an unregistered tool or any `cwd` argument.
fn admit(tool: &str, arguments: &Value) -> Result<Rule, ErrorCode> {
    let rule = rule(tool).ok_or(ErrorCode::Forbidden)?;
    if arguments.get("cwd").is_some() {
        return Err(ErrorCode::Forbidden);
    }
    Ok(rule)
}

/// Place, label and cap a scoped `create_agent`.
///
/// The Workspace defaults to the caller's own Workspace for subtree callers (`inherited`) and to
/// the root for Bot sessions. Created Agents carry `BOT_SCOPE_LABEL = root`, so their sessions
/// receive derived credentials instead of the host token.
/// # Errors
/// Returns `Forbidden` for a foreign Workspace, reserved labels, features or modes outside the
/// cap, and `InvalidMessage` for malformed arguments.
fn rewrite_create_agent(
    scope: &Scope,
    inherited: Option<&str>,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    let root = scope.caller.root.as_str();
    let workspace = match arguments.get("workspaceId") {
        Some(workspace) => text(workspace)?,
        None => match scope.caller.kind {
            CallerKind::Subtree { .. } => inherited.unwrap_or(root).to_owned(),
            CallerKind::Bot { .. } => root.to_owned(),
        },
    };
    scope.contains(Some(&workspace))?;
    arguments["workspaceId"] = json!(workspace);
    cap_mode(&scope.grant.permissions, arguments)?;
    features(arguments)?;
    reserved_labels(arguments)?;
    let labels = object(arguments, "labels")?;
    labels.insert(BOT_SCOPE_LABEL.to_owned(), json!(root));
    Ok(())
}

/// Cap `update_agent` settings by the resolved `agent`'s provider and refuse reserved labels.
/// # Errors
/// Returns `Forbidden` for reserved labels, features or a mode outside the cap.
fn cap_update_agent(scope: &Scope, agent: &Value, arguments: &Value) -> Result<(), ErrorCode> {
    reserved_labels(arguments)?;
    features(arguments)?;
    match arguments.pointer("/settings/modeId") {
        Some(mode) => cap_target(&scope.grant.permissions, agent, mode),
        None => Ok(()),
    }
}

/// Cap `set_agent_mode` by the resolved `agent`'s provider.
/// # Errors
/// Returns `Forbidden` for a mode outside the cap.
fn cap_agent_mode(scope: &Scope, agent: &Value, arguments: &Value) -> Result<(), ErrorCode> {
    cap_target(&scope.grant.permissions, agent, &arguments["modeId"])
}

/// Require `mode` to be allowed for the provider of existing `agent`.
fn cap_target(permissions: &BotPermissions, agent: &Value, mode: &Value) -> Result<(), ErrorCode> {
    let provider = text(&agent["provider"])?;
    permissions
        .mode(&provider, Some(&text(mode)?))
        .map(drop)
        .map_err(|_| ErrorCode::Forbidden)
}

/// Require `create_agent`'s provider and `settings.modeId` to be within `permissions`' cap,
/// selecting the provider's default mode when none is requested.
/// # Errors
/// Returns `InvalidMessage` for a malformed provider and `Forbidden` outside the cap.
fn cap_mode(permissions: &BotPermissions, arguments: &mut Value) -> Result<(), ErrorCode> {
    let (provider, _) = creation::provider_model(&arguments["provider"], true)?;
    let requested = arguments
        .pointer("/settings/modeId")
        .and_then(Value::as_str);
    let mode = permissions
        .mode(provider, requested)
        .map_err(|_| ErrorCode::Forbidden)?;
    arguments["settings"]["modeId"] = json!(mode);
    Ok(())
}

/// Place a scoped `create_schedule` at its provider's default mode and in the Bot scope.
///
/// The provider defaults to the subtree caller's own (`inherited`). The mode and scope are passed
/// as `modeId` and `botScope`, which the tool schema does not let callers supply themselves.
/// # Errors
/// Returns `InvalidMessage` for a missing or malformed provider and `Forbidden` for a provider
/// outside the cap.
fn rewrite_create_schedule(
    scope: &Scope,
    inherited: Option<&Value>,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    if arguments.get("provider").is_none() {
        arguments["provider"] = inherited
            .filter(|provider| provider.is_string())
            .cloned()
            .ok_or(ErrorCode::InvalidMessage)?;
    }
    let (provider, _) = creation::provider_model(&arguments["provider"], false)?;
    let mode = scope
        .grant
        .permissions
        .mode(provider, None)
        .map_err(|_| ErrorCode::Forbidden)?;
    arguments["modeId"] = json!(mode);
    arguments["botScope"] = json!(scope.caller.root);
    Ok(())
}

/// Return the scoped Workspace of a heartbeat's target, the subtree caller Agent `context`.
/// # Errors
/// Returns `Forbidden` for Bot sessions, which have no targetable Agent, and for a caller
/// outside the grant; `InvalidMessage` without caller context.
fn heartbeat_workspace<'a>(
    scope: &Scope,
    context: Option<&'a Value>,
) -> Result<&'a str, ErrorCode> {
    if matches!(scope.caller.kind, CallerKind::Bot { .. }) {
        return Err(ErrorCode::Forbidden);
    }
    let workspace = context
        .and_then(|agent| agent["workspaceId"].as_str())
        .ok_or(ErrorCode::InvalidMessage)?;
    scope.contains(Some(workspace))?;
    Ok(workspace)
}

/// Require a new-Agent `schedule`'s current provider and mode to be within `permissions`' cap.
///
/// A task without a stored mode counts as the provider's default mode. Heartbeats target an
/// existing Agent and have no mode; schedule tools other than `delete_heartbeat` refuse them.
/// # Errors
/// Returns `Forbidden` outside the cap and `InvalidMessage` for a malformed task.
fn cap_schedule(permissions: &BotPermissions, schedule: &Value) -> Result<(), ErrorCode> {
    if schedule["target"]["type"] != "new-agent" {
        return Ok(());
    }
    let config = &schedule["target"]["config"];
    permissions
        .mode(&text(&config["provider"])?, config["modeId"].as_str())
        .map(drop)
        .map_err(|_| ErrorCode::Forbidden)
}

/// Cap the provider and mode an `update_schedule` patch leaves on `schedule`.
///
/// `mode: null` and a provider change without a mode select the provider's default mode, which
/// is written into the patch; patches without `mode` or `provider` keep the checked current mode.
/// # Errors
/// Returns `Forbidden` outside the cap and `InvalidMessage` for a malformed provider.
fn cap_schedule_update(
    permissions: &BotPermissions,
    schedule: &Value,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    let provider = match arguments.get("provider") {
        Some(patch) => creation::provider_model(patch, false)?.0.to_owned(),
        None if arguments.get("mode").is_some() => text(&schedule["target"]["config"]["provider"])?,
        None => return Ok(()),
    };
    let requested = arguments.get("mode").and_then(Value::as_str);
    let mode = permissions
        .mode(&provider, requested)
        .map_err(|_| ErrorCode::Forbidden)?;
    arguments["mode"] = json!(mode);
    Ok(())
}

/// Refuse every `settings.features` entry except `fast_mode`.
fn features(arguments: &Value) -> Result<(), ErrorCode> {
    let refused = arguments
        .pointer("/settings/features")
        .and_then(Value::as_object)
        .is_some_and(|features| features.keys().any(|feature| feature != ALLOWED_FEATURE));
    if refused {
        Err(ErrorCode::Forbidden)
    } else {
        Ok(())
    }
}

/// Require a target's `workspace` to be in scope and replace `arguments[key]` with its
/// canonical `id`.
/// # Errors
/// Returns `Forbidden` for a target outside the grant's Workspaces.
fn retarget(
    scope: &Scope,
    key: &str,
    workspace: Option<&str>,
    id: &str,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    scope.contains(workspace)?;
    arguments[key] = json!(id);
    Ok(())
}

/// Confine a `create_workspace` to a new worktree of the `root` Workspace's project.
///
/// Replaying an idempotency key could return an earlier creation and adopt an arbitrary
/// Workspace, so keys are refused; the capacity is checked before anything is created.
/// # Errors
/// Returns `Forbidden` for non-worktree isolation or an idempotency key, `ResourceExhausted`
/// when the Bot already records its maximum of worktrees.
fn rewrite_create_workspace(
    scope: &Scope,
    root: &Value,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    if arguments["isolation"] != "worktree" || arguments.get("idempotencyKey").is_some() {
        return Err(ErrorCode::Forbidden);
    }
    // The grant always contains the root itself besides the Bot's recorded worktrees.
    if scope.grant.workspaces.len().saturating_sub(1) >= BOT_WORKSPACES {
        return Err(ErrorCode::ResourceExhausted);
    }
    arguments["path"] = root["workspaceDirectory"].clone();
    arguments["projectId"] = root["projectId"].clone();
    Ok(())
}

/// Resolve `arguments.terminalId` among the terminals of all Workspaces and confine it.
async fn terminal(state: &Shared, scope: &Scope, arguments: &mut Value) -> Result<(), ErrorCode> {
    let requested = text(&arguments["terminalId"])?;
    let listed = rpc::execute(state, "terminal.list.request", json!({})).await?;
    let terminal = listed["terminals"]
        .as_array()
        .ok_or(ErrorCode::InvalidMessage)?
        .iter()
        .find(|terminal| terminal["id"] == requested.as_str())
        .ok_or(ErrorCode::Forbidden)?;
    let workspace = text(&terminal["workspaceId"])?;
    retarget(scope, "terminalId", Some(&workspace), &requested, arguments)?;
    active(state, &workspace).await.map(drop)
}

/// Resolve schedule `arguments.id`, confine it to the scoped Workspaces and return its document.
async fn schedule(
    state: &Shared,
    scope: &Scope,
    arguments: &mut Value,
) -> Result<Value, ErrorCode> {
    let mut snapshot = rpc::execute(
        state,
        "schedule.inspect.request",
        json!({"scheduleId":arguments["id"]}),
    )
    .await?;
    let schedule = snapshot["schedule"].take();
    let id = text(&schedule["id"])?;
    retarget(
        scope,
        "id",
        schedule["workspaceId"].as_str(),
        &id,
        arguments,
    )?;
    active(state, &text(&schedule["workspaceId"])?).await?;
    Ok(schedule)
}

/// Return the descriptor of unarchived Workspace `id`.
/// # Errors
/// Returns `Forbidden` for a missing or archived Workspace and propagates lookup failures.
async fn active(state: &Shared, id: &str) -> Result<Value, ErrorCode> {
    let mut target =
        rpc::execute(state, "workspace.get.request", json!({"workspaceId":id})).await?;
    match target.get_mut("workspace").map(Value::take) {
        Some(workspace) if target["state"] == "ready" && workspace.is_object() => Ok(workspace),
        _ => Err(ErrorCode::Forbidden),
    }
}

/// Return `value` as owned text.
fn text(value: &Value) -> Result<String, ErrorCode> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or(ErrorCode::InvalidMessage)
}

/// Return `arguments[key]` as a mutable object, inserting an empty one when absent.
fn object<'a>(
    arguments: &'a mut Value,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ErrorCode> {
    arguments
        .as_object_mut()
        .ok_or(ErrorCode::InvalidMessage)?
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(ErrorCode::InvalidMessage)
}

#[cfg(test)]
mod tests;
