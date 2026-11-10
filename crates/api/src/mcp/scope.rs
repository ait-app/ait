//! Authorization and argument rewriting for Bot-scoped MCP callers.
//!
//! A scoped caller may reach only its grant's Workspaces: the root and the unarchived worktrees
//! its Bot created. Every target id is resolved through the existing capability RPCs and replaced
//! by the canonical id, so later RPCs never re-resolve a prefix or title to another target.

use std::collections::BTreeSet;

use bot::mcp::{Caller, CallerKind, Grant};
use domain::bot::BOT_SCOPE_LABEL;
use domain::bot::permissions::BotPermissions;
use model::ErrorCode;
use serde_json::{Map, Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::tools::creation;

/// Label namespace owned by Ait; scoped callers may write no label in it.
const AIT_LABEL_PREFIX: &str = "ait.";
/// The only native feature a scoped caller may set; others can change execution permissions.
const ALLOWED_FEATURE: &str = "fast_mode";
/// Refusal for running or keeping a task without a stored mode, whose native default the cap
/// cannot vouch for.
const UNSET_MODE: &str = "This schedule has no explicit mode, so it would run at the \
    provider's native default, which may exceed the Workspace Bot mode cap; ask the user to set \
    an allowed mode for it in Ait";
/// Refusal for editing a task the user created; its prompt and configuration stay the user's.
const USER_SCHEDULE: &str = "Only schedules created by the Workspace Bot can be edited; ask the \
    user to change this schedule in Ait";

/// Why a tool call failed: a stable error code, or a scope refusal that guides the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    /// A stable error code; its message is the result text.
    Code(ErrorCode),
    /// A scope refusal whose text tells the caller how to proceed.
    Refused(&'static str),
}

impl From<ErrorCode> for Failure {
    fn from(code: ErrorCode) -> Self {
        Self::Code(code)
    }
}

impl Failure {
    /// The text returned to the caller.
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Code(code) => code.message(),
            Self::Refused(text) => text,
        }
    }
}

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
        "list_workspace_scripts"
        | "start_workspace_script"
        | "stop_workspace_script"
        | "get_workspace_script_run"
        | "wait_for_workspace_script" => Rule::Workspace,
        "list_terminals" | "create_terminal" => Rule::RootWorkspace,
        "kill_terminal" | "capture_terminal" | "send_terminal_keys" => Rule::Terminal,
        "inspect_schedule" | "pause_schedule" | "delete_schedule" | "schedule_logs"
        | "delete_heartbeat" => Rule::Schedule,
        "run_schedule_once" | "resume_schedule" => Rule::ScheduleControl,
        "update_schedule" => Rule::UpdateSchedule,
        "create_schedule" => Rule::CreateSchedule,
        "create_heartbeat" => Rule::CreateHeartbeat,
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
/// labels, features or modes outside the cap (including a schedule's effective mode), and Bot
/// heartbeats; a guided refusal for editing a schedule not created by a Bot or for running or
/// resuming such a schedule without a stored mode; and propagates target lookup failures.
pub(super) async fn confine(
    state: &Shared,
    scope: &Scope,
    tool: &str,
    arguments: &mut Value,
    context: Option<&Value>,
) -> Result<(), Failure> {
    let root = scope.caller.root.as_str();
    let confined = match admit(tool, arguments)? {
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
            let permissions = &scope.grant.permissions;
            match rule {
                Rule::ScheduleControl => cap_schedule(permissions, &schedule)?,
                Rule::UpdateSchedule if schedule.get("botScope").is_none() => {
                    return Err(Failure::Refused(USER_SCHEDULE));
                }
                // An update is checked by the configuration it leaves behind.
                Rule::UpdateSchedule
                    if !cap_schedule_update(permissions, &schedule, arguments)? =>
                {
                    cap_schedule(permissions, &schedule)?;
                }
                _ => {}
            }
            Ok(())
        }
    };
    Ok(confined?)
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

/// Refuse labels starting with the reserved `prefix`: no MCP caller may write `ait.bot.` labels,
/// and scoped callers may write no `ait.` label at all.
/// # Errors
/// Returns `Forbidden` when `arguments.labels` names a reserved label.
pub(super) fn reserved_labels(arguments: &Value, prefix: &str) -> Result<(), ErrorCode> {
    let reserved = arguments
        .get("labels")
        .and_then(Value::as_object)
        .is_some_and(|labels| labels.keys().any(|key| key.starts_with(prefix)));
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
    reserved_labels(arguments, AIT_LABEL_PREFIX)?;
    let labels = object(arguments, "labels")?;
    labels.insert(BOT_SCOPE_LABEL.to_owned(), json!(root));
    Ok(())
}

/// Cap `update_agent` settings by the resolved `agent`'s provider and refuse reserved labels.
/// # Errors
/// Returns `Forbidden` for reserved labels, features or a mode outside the cap.
fn cap_update_agent(scope: &Scope, agent: &Value, arguments: &Value) -> Result<(), ErrorCode> {
    reserved_labels(arguments, AIT_LABEL_PREFIX)?;
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
/// A Bot-created task (`botScope`) without a stored mode runs at the cap's default mode, so only
/// its provider must be listed. Any other mode-less task would run at the provider's native
/// default, which the cap cannot vouch for. Heartbeat targets have no mode and pass here;
/// `operate` rejects them for running, resuming and updating.
/// # Errors
/// Returns `Forbidden` outside the cap, the `UNSET_MODE` refusal for a mode-less task not created
/// by a Bot, and `InvalidMessage` for a malformed task.
fn cap_schedule(permissions: &BotPermissions, schedule: &Value) -> Result<(), Failure> {
    if schedule["target"]["type"] != "new-agent" {
        return Ok(());
    }
    let config = &schedule["target"]["config"];
    let mode = config["modeId"].as_str();
    permissions
        .mode(&text(&config["provider"])?, mode)
        .map_err(|_| ErrorCode::Forbidden)?;
    if mode.is_none() && schedule.get("botScope").is_none() {
        return Err(Failure::Refused(UNSET_MODE));
    }
    Ok(())
}

/// Cap the provider and mode an `update_schedule` patch sets on `schedule`.
///
/// `mode: null` and a provider change without a mode select the provider's default mode, which
/// is written into the patch, so a patched task always stores an allowed mode.
/// # Returns
/// Whether the patch sets a provider or mode and was checked; otherwise the current
/// configuration stays in effect and must be checked by the caller.
/// # Errors
/// Returns `Forbidden` outside the cap and `InvalidMessage` for a malformed provider.
fn cap_schedule_update(
    permissions: &BotPermissions,
    schedule: &Value,
    arguments: &mut Value,
) -> Result<bool, ErrorCode> {
    let provider = match arguments.get("provider") {
        Some(patch) => creation::provider_model(patch, false)?.0.to_owned(),
        None if arguments.get("mode").is_some() => text(&schedule["target"]["config"]["provider"])?,
        None => return Ok(false),
    };
    let requested = arguments.get("mode").and_then(Value::as_str);
    let mode = permissions
        .mode(&provider, requested)
        .map_err(|_| ErrorCode::Forbidden)?;
    arguments["mode"] = json!(mode);
    Ok(true)
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
