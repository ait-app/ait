//! Bot-only tools: long-term memory and a bounded overview of the Bot's Workspace.

use domain::bot::BOT_SCOPE_LABEL;
use domain::bot::permissions::BotPermissions;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;
use crate::mcp::Scope;
use crate::mcp::scope::Failure;

/// Largest serialized overview in bytes.
const MAX_BYTES: usize = 24 * 1024;
/// Largest `lastMessage` kept per Agent, in bytes.
const MAX_MESSAGE: usize = 1024;
/// Largest name or error kept per schedule, in bytes.
const MAX_LABEL: usize = 256;
const MAX_BOT_AGENTS: usize = 20;
const MAX_ATTENTION: usize = 10;
const MAX_SCHEDULES: usize = 20;
/// Overview lists that may shrink to fit, as JSON pointers.
const LISTS: [&str; 3] = ["/botAgents", "/otherAgents/attention", "/schedules/bot"];
/// Agents fetched to count and sample the Agents the Bot did not start.
const AGENT_PAGE: u64 = 200;
/// Refusal for an update made against a stale revision.
const STALE_REVISION: &str = "The memory changed since you read it; call bot_memory_read again \
    and retry bot_memory_update with the new revision";

/// Whether `tool` is one of the Bot-only tools handled by [`execute`].
pub(super) fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "bot_memory_read" | "bot_memory_update" | "workspace_overview"
    )
}

/// Run Bot-only `tool` for the Bot owning `scope`'s root Workspace.
/// # Errors
/// Returns `Forbidden` without a scoped credential, the stale-revision refusal when `expected`
/// no longer matches, and existing Bot, Agent, schedule or checkout failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &str,
    arguments: Value,
    scope: Option<&Scope>,
) -> Result<Value, Failure> {
    let scope = scope.ok_or(ErrorCode::Forbidden)?;
    let root = scope.caller.root.as_str();
    match tool {
        "bot_memory_read" => Ok(memory(state, root).await?),
        "bot_memory_update" => {
            let params = json!({"workspaceId":root,"text":arguments["text"],
                "expected":arguments["expected"]});
            match rpc::execute(state, "bot.memory.update.request", params).await {
                Err(ErrorCode::BotConflict) => Err(Failure::Refused(STALE_REVISION)),
                other => Ok(other?),
            }
        }
        _ => Ok(overview(state, scope).await?),
    }
}

async fn memory(state: &Shared, root: &str) -> Result<Value, ErrorCode> {
    rpc::execute(state, "bot.memory.get.request", json!({"workspaceId":root})).await
}

/// Gather the sources of `scope`'s overview and project them within the size bound.
async fn overview(state: &Shared, scope: &Scope) -> Result<Value, ErrorCode> {
    let root = scope.caller.root.as_str();
    let workspaces = &scope.grant.workspaces;
    let descriptor = rpc::execute(state, "workspace.get.request", json!({"workspaceId":root}))
        .await?["workspace"]
        .take();
    let checkout = rpc::execute(
        state,
        "checkout.status.get.request",
        json!({"cwd":descriptor["workspaceDirectory"]}),
    )
    .await?;
    let agents = |filter: Value, limit: u64| {
        let mut filter = filter;
        filter["workspaceIds"] = json!(workspaces);
        rpc::execute(
            state,
            "agent.list.request",
            json!({"filter":filter,"sort":[{"key":"updated_at","direction":"desc"}],
                "page":{"limit":limit}}),
        )
    };
    let own = agents(
        json!({"labels":{BOT_SCOPE_LABEL:root}}),
        MAX_BOT_AGENTS as u64 + 1,
    )
    .await?;
    let all = agents(json!({}), AGENT_PAGE).await?;
    let schedules = rpc::execute(state, "schedule.list.request", json!({})).await?;
    let memory = memory(state, root).await?;
    Ok(project(&Sources {
        root,
        checkout: &checkout,
        own: &entries(&own)?,
        all: &entries(&all)?,
        all_complete: all["pageInfo"]["nextCursor"].is_null(),
        schedules: schedules["schedules"]
            .as_array()
            .ok_or(ErrorCode::InvalidMessage)?,
        workspaces,
        memory: &memory,
        permissions: &scope.grant.permissions,
    }))
}

fn entries(page: &Value) -> Result<Vec<Value>, ErrorCode> {
    Ok(page["entries"]
        .as_array()
        .ok_or(ErrorCode::InvalidMessage)?
        .iter()
        .map(|entry| entry["agent"].clone())
        .collect())
}

/// Everything the overview reports on, as read from the existing capabilities.
pub(super) struct Sources<'a> {
    /// Root Workspace of the Bot.
    pub(super) root: &'a str,
    /// `checkout.status.get.request` result of the root Workspace.
    pub(super) checkout: &'a Value,
    /// Agents carrying the Bot scope label, newest first.
    pub(super) own: &'a [Value],
    /// Unarchived Agents of all scoped Workspaces, newest first.
    pub(super) all: &'a [Value],
    /// Whether `all` lists every such Agent.
    pub(super) all_complete: bool,
    /// Schedules of every Workspace, which are filtered to `workspaces` here.
    pub(super) schedules: &'a [Value],
    /// Workspaces in the Bot's scope.
    pub(super) workspaces: &'a std::collections::BTreeSet<String>,
    /// `bot.memory.get.request` result.
    pub(super) memory: &'a Value,
    /// The Bot's current permissions.
    pub(super) permissions: &'a BotPermissions,
}

/// Project `sources` into the overview document of at most [`MAX_BYTES`] bytes.
///
/// Lists are capped first. If the document is still too large, the Agents' `lastMessage` fields
/// go, then list entries, which sets `truncated`.
pub(super) fn project(sources: &Sources<'_>) -> Value {
    let root = sources.root;
    let mut truncated = !sources.all_complete;
    let mut bot_agents: Vec<Value> = sources.own.iter().map(agent).collect();
    truncated |= cap(&mut bot_agents, MAX_BOT_AGENTS);
    let others: Vec<_> = sources
        .all
        .iter()
        .filter(|agent| agent["labels"][BOT_SCOPE_LABEL] != root)
        .collect();
    let mut attention: Vec<Value> = others
        .iter()
        .filter(|agent| agent["requiresAttention"] == true)
        .map(|agent| {
            json!({"id":agent["id"],"title":agent["title"],"status":agent["status"],
                "attentionReason":agent["attentionReason"]})
        })
        .collect();
    truncated |= cap(&mut attention, MAX_ATTENTION);
    let (mut own_schedules, mut other_count) = (Vec::new(), 0);
    for schedule in sources.schedules.iter().filter(|schedule| {
        schedule["workspaceId"]
            .as_str()
            .is_some_and(|id| sources.workspaces.contains(id))
    }) {
        if schedule["botScope"] == root {
            own_schedules.push(
                json!({"id":schedule["id"],"name":clip(&schedule["name"], MAX_LABEL),
                "status":schedule["status"],"nextRunAt":schedule["nextRunAt"],
                "lastRunAt":schedule["lastRunAt"]}),
            );
        } else {
            other_count += 1;
        }
    }
    truncated |= cap(&mut own_schedules, MAX_SCHEDULES);
    let text = sources.memory["text"].as_str().unwrap_or_default();
    let mut overview = json!({
        "workspace":{"id":root,"branch":sources.checkout["currentBranch"],
            "dirty":sources.checkout["isDirty"]},
        "botAgents":bot_agents,
        "otherAgents":{"count":others.len(),"attention":attention},
        "schedules":{"bot":own_schedules,"otherCount":other_count},
        "notifications":null,
        "memory":{"revision":sources.memory["revision"],"bytes":text.len()},
        "permissions":sources.permissions,
        "truncated":truncated,
    });
    if overview.to_string().len() > MAX_BYTES {
        for agent in overview["botAgents"].as_array_mut().into_iter().flatten() {
            if let Some(agent) = agent.as_object_mut() {
                agent.remove("lastMessage");
            }
        }
    }
    while overview.to_string().len() > MAX_BYTES {
        overview["truncated"] = json!(true);
        let longest = LISTS.iter().max_by_key(|path| {
            overview
                .pointer(path)
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        });
        match longest.and_then(|path| overview.pointer_mut(path)) {
            Some(Value::Array(list)) if !list.is_empty() => {
                list.pop();
            }
            _ => break,
        }
    }
    overview
}

/// Keep the first `limit` entries of `list`, reporting whether any were dropped.
fn cap(list: &mut Vec<Value>, limit: usize) -> bool {
    let dropped = list.len() > limit;
    list.truncate(limit);
    dropped
}

fn agent(agent: &Value) -> Value {
    let mut summary = json!({"id":agent["id"],"title":agent["title"],
        "provider":agent["provider"],"status":agent["status"],
        "requiresAttention":agent["requiresAttention"],"updatedAt":agent["updatedAt"]});
    if agent["lastMessage"].is_string() {
        summary["lastMessage"] = clip(&agent["lastMessage"], MAX_MESSAGE);
    }
    summary
}

/// Return text `value` cut to at most `limit` bytes on a character boundary; other values pass.
fn clip(value: &Value, limit: usize) -> Value {
    match value.as_str() {
        Some(text) if text.len() > limit => {
            let end = (0..=limit)
                .rev()
                .find(|&index| text.is_char_boundary(index))
                .unwrap_or(0);
            json!(&text[..end])
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests;
