//! Bot-only tools: long-term memory and a bounded overview of the Bot's Workspace.

use std::collections::HashMap;

use bot::mcp::CallerKind;
use bot::recall::{Sight, spill};
use domain::bot::BOT_SCOPE_LABEL;
use domain::bot::permissions::BotPermissions;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;
use crate::mcp::Scope;
use crate::mcp::scope::Failure;

use super::recall::{external, restricted_waiting, sees, sees_agent, sight};

/// Largest serialized overview in bytes.
const MAX_BYTES: usize = 24 * 1024;
/// Largest name or error kept per schedule, in bytes.
const MAX_LABEL: usize = 256;
const MAX_BOT_AGENTS: usize = 20;
const MAX_ATTENTION: usize = 10;
const MAX_SCHEDULES: usize = 20;
const MAX_NOTIFICATIONS: usize = 20;
/// Open tickets listed, as ADR-274 bounds them.
const MAX_TICKETS: usize = 20;
/// Longest `summary` kept per notification and `text` per latest ticket report, in characters;
/// a longer one spills to the resource that returns it whole.
const MAX_SUMMARY: usize = 512;
/// Every notification the log keeps, read for an external conversation, whose own entries the
/// overview filters out of the newest ones.
const LOG_PAGE: usize = 128;
/// Notification fields copied into the overview when present.
const NOTIFICATION_FIELDS: [&str; 8] = [
    "kind",
    "title",
    "workspaceId",
    "agentId",
    "scheduleId",
    "runId",
    "terminalId",
    "at",
];
/// Overview lists that may shrink to fit, as JSON pointers.
const LISTS: [&str; 5] = [
    "/botAgents",
    "/otherAgents/attention",
    "/schedules/bot",
    "/notifications",
    "/tickets",
];
/// Texts reduced to their length and read pointer in every entry of a list before entries go:
/// the list, the pointer of the object within an entry, and its field.
const TEXTS: [(&str, &str, &str); 2] = [
    ("notifications", "", "summary"),
    ("tickets", "/latestReport", "text"),
];
/// Agents fetched to count and sample the Agents the Bot did not start, and the Agents read for
/// a caller that reaches only its own work, whose own Agents the overview and `list_agents`
/// filter out of them.
pub(super) const AGENT_PAGE: u64 = 200;
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
    let sight = sight(state, scope, None).await?;
    let own_limit = if external(sight.as_ref()) {
        AGENT_PAGE
    } else {
        MAX_BOT_AGENTS as u64 + 1
    };
    let own = agents(json!({"labels":{BOT_SCOPE_LABEL:root}}), own_limit).await?;
    let all = agents(json!({}), AGENT_PAGE).await?;
    let schedules = rpc::execute(state, "schedule.list.request", json!({})).await?;
    let memory = memory(state, root).await?;
    let notifications = unseen(state, scope, sight.as_ref()).await?;
    let tickets = open_tickets(state, root).await?;
    Ok(project(&Sources {
        root,
        checkout: &checkout,
        own: &entries(&own)?,
        own_complete: own["pageInfo"]["nextCursor"].is_null(),
        all: &entries(&all)?,
        all_complete: all["pageInfo"]["nextCursor"].is_null(),
        schedules: schedules["schedules"]
            .as_array()
            .ok_or(ErrorCode::InvalidMessage)?,
        workspaces,
        memory: &memory,
        notifications: &notifications,
        tickets: &tickets,
        sight,
        permissions: &scope.grant.permissions,
    }))
}

/// Read the Bot's open tickets, one more than the overview keeps so it can report truncation;
/// `null` once the Bot is gone.
/// # Errors
/// Propagates other Bot read failures.
async fn open_tickets(state: &Shared, root: &str) -> Result<Value, ErrorCode> {
    let params = json!({"workspaceId":root,"limit":MAX_TICKETS + 1});
    match rpc::execute(state, "bot.ticket.list.request", params).await {
        Err(ErrorCode::BotNotFound) => Ok(Value::Null),
        other => other,
    }
}

/// Read the notifications the calling Bot conversation has not had in front of it in a
/// completed turn, one more than the overview keeps so it can report truncation.
///
/// An external conversation reads every kept entry, because the overview keeps only those it
/// originated. Returns `null` for callers without a conversation and for a conversation the
/// Bot no longer has, such as one closed while its session still answered.
/// # Errors
/// Propagates other Bot read failures.
async fn unseen(state: &Shared, scope: &Scope, sight: Option<&Sight>) -> Result<Value, ErrorCode> {
    let CallerKind::Bot {
        conversation_id, ..
    } = &scope.caller.kind
    else {
        return Ok(Value::Null);
    };
    let limit = if external(sight) {
        LOG_PAGE
    } else {
        MAX_NOTIFICATIONS + 1
    };
    let params = json!({"workspaceId":scope.caller.root,"unseenBy":conversation_id,
        "limit":limit});
    match rpc::execute(state, "bot.notifications.list.request", params).await {
        Err(ErrorCode::BotNotFound) => Ok(Value::Null),
        other => other,
    }
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
    /// Whether `own` lists every such Agent.
    pub(super) own_complete: bool,
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
    /// `bot.notifications.list.request` result for the calling conversation, or `null`.
    pub(super) notifications: &'a Value,
    /// `bot.ticket.list.request` result of the Bot's open tickets, or `null`.
    pub(super) tickets: &'a Value,
    /// What the calling Bot conversation may read and reach; it also marks the tickets it asked
    /// for. `None` reads everything and marks nothing.
    pub(super) sight: Option<Sight>,
    /// The Bot's current permissions.
    pub(super) permissions: &'a BotPermissions,
}

/// Project `sources` into the overview document of at most [`MAX_BYTES`] bytes.
///
/// Lists are capped first, and long texts spill to a read pointer (ADR-280). If the document is
/// still too large, the notifications' `summary` and the tickets' latest report `text` shrink
/// to their length and pointer, then list entries go, which sets
/// `truncated` and lists in `more` where each list reads whole. An external conversation sees
/// only the notifications it originated and the Agents of its own tasks (ADR-280 §4): other
/// tickets lose their latest report and show a count of Agents instead of their ids, and the
/// Agents the Bot did not start are only counted.
pub(super) fn project(sources: &Sources<'_>) -> Value {
    let root = sources.root;
    let sight = sources.sight.as_ref();
    let mut truncated = !sources.all_complete || !sources.own_complete;
    let mut bot_agents: Vec<Value> = sources
        .own
        .iter()
        .filter(|agent| sees_agent(sight, agent))
        .map(agent)
        .collect();
    truncated |= cap(&mut bot_agents, MAX_BOT_AGENTS);
    let others: Vec<_> = sources
        .all
        .iter()
        .filter(|agent| agent["labels"][BOT_SCOPE_LABEL] != root)
        .collect();
    let mut attention: Vec<Value> = others
        .iter()
        .filter(|agent| agent["requiresAttention"] == true && sees_agent(sight, agent))
        .map(|agent| {
            json!({"id":agent["id"],"title":agent["title"],"status":agent["status"],
                "attentionReason":agent["attentionReason"]})
        })
        .collect();
    truncated |= cap(&mut attention, MAX_ATTENTION);
    let (mut own_schedules, other_count) = schedules(sources);
    truncated |= cap(&mut own_schedules, MAX_SCHEDULES);
    let notifications = sources.notifications["notifications"]
        .as_array()
        .map(|entries| {
            let mut kept: Vec<Value> = entries
                .iter()
                .filter(|entry| sees(sight, &entry["notification"]["conversationId"]))
                .map(notification)
                .collect();
            truncated |= cap(&mut kept, MAX_NOTIFICATIONS);
            kept
        });
    let tickets = sources.tickets["tickets"].as_array().map(|listed| {
        let mut kept = ticket_entries(sources, listed);
        truncated |= cap(&mut kept, MAX_TICKETS);
        kept
    });
    let text = sources.memory["text"].as_str().unwrap_or_default();
    let mut overview = json!({
        "workspace":{"id":root,"branch":sources.checkout["currentBranch"],
            "dirty":sources.checkout["isDirty"]},
        "botAgents":bot_agents,
        "otherAgents":{"count":others.len(),"attention":attention},
        "schedules":{"bot":own_schedules,"otherCount":other_count},
        "notifications":notifications,
        "tickets":tickets,
        "memory":{"revision":sources.memory["revision"],"bytes":text.len()},
        "permissions":sources.permissions,
        "truncated":truncated,
    });
    if overview.to_string().len() > MAX_BYTES {
        for (list, object, field) in TEXTS {
            for entry in overview[list].as_array_mut().into_iter().flatten() {
                let reference = text_reference(list, entry);
                if let Some(text) = entry
                    .pointer_mut(object)
                    .and_then(|object| object.get_mut(field))
                {
                    shrink(text, reference);
                }
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
    if overview["truncated"] == true {
        overview["more"] = json!({"tickets":{"read":"tickets"},
            "notifications":{"read":"notifications"},"agents":{"tool":"list_agents"},
            "schedules":{"tool":"list_schedules"}});
    }
    overview
}

/// The Bot's own schedules in the scoped Workspaces, projected, and how many others they hold.
fn schedules(sources: &Sources<'_>) -> (Vec<Value>, usize) {
    let (mut own, mut other_count) = (Vec::new(), 0);
    let scoped = sources.schedules.iter().filter(|schedule| {
        schedule["workspaceId"]
            .as_str()
            .is_some_and(|id| sources.workspaces.contains(id))
    });
    for schedule in scoped {
        if schedule["botScope"] == sources.root {
            own.push(
                json!({"id":schedule["id"],"name":clip(&schedule["name"], MAX_LABEL),
                "status":schedule["status"],"nextRunAt":schedule["nextRunAt"],
                "lastRunAt":schedule["lastRunAt"]}),
            );
        } else {
            other_count += 1;
        }
    }
    (own, other_count)
}

/// The `bot_read` resource where the whole of `list` `entry`'s text reads.
fn text_reference(list: &str, entry: &Value) -> Value {
    match list {
        "tickets" => json!(format!(
            "ticket/{}/report/{}",
            entry["id"].as_str().unwrap_or_default(),
            entry["latestReport"]["sequence"]
        )),
        _ => entry["read"].clone(),
    }
}

/// Reduce text field `text`, inline or already spilled, to its length in characters and the
/// `reference` resource that reads it whole.
fn shrink(text: &mut Value, reference: Value) {
    let chars = match text {
        Value::String(text) => text.chars().count(),
        Value::Object(spilled) => spilled
            .get("chars")
            .and_then(Value::as_u64)
            .and_then(|chars| usize::try_from(chars).ok())
            .unwrap_or_default(),
        _ => return,
    };
    let mut shrunk = serde_json::Map::new();
    shrunk.insert("chars".to_owned(), json!(chars));
    shrunk.insert("read".to_owned(), reference);
    *text = Value::Object(shrunk);
}

/// Keep the first `limit` entries of `list`, reporting whether any were dropped.
fn cap(list: &mut Vec<Value>, limit: usize) -> bool {
    let dropped = list.len() > limit;
    list.truncate(limit);
    dropped
}

/// An Agent of the Bot as the overview lists it; its result reads through `get_agent_activity`.
fn agent(agent: &Value) -> Value {
    json!({"id":agent["id"],"title":agent["title"],"provider":agent["provider"],
        "status":agent["status"],"requiresAttention":agent["requiresAttention"],
        "updatedAt":agent["updatedAt"]})
}

/// Project the listed open `tickets`, joining each assigned Agent's live status from the Agent
/// directory the overview read.
///
/// An Agent missing from a complete directory read is archived (`"status":"archived"`); from an
/// incomplete one its status is unknown (`null`). A ticket the viewer may not read keeps its
/// title and status and the number of its Agents, but not their ids or its latest report.
fn ticket_entries(sources: &Sources<'_>, tickets: &[Value]) -> Vec<Value> {
    let directory: HashMap<&str, &Value> = sources
        .all
        .iter()
        .chain(sources.own)
        .filter_map(|agent| Some((agent["id"].as_str()?, agent)))
        .collect();
    let missing = if sources.all_complete {
        json!("archived")
    } else {
        Value::Null
    };
    let sight = sources.sight.as_ref();
    let conversation = sight
        .and_then(|sight| sight.viewer.conversation())
        .map(|id| id.to_string());
    tickets
        .iter()
        .map(|ticket| {
            let assignments = ticket["assignments"].as_array().map_or(&[][..], Vec::as_slice);
            let readable = sees(sight, &ticket["conversationId"]);
            let agents = if readable {
                assigned(assignments, &directory, &missing)
            } else {
                json!(assignments.len())
            };
            let id = ticket["id"].as_str().unwrap_or_default();
            let report = &ticket["latestReport"];
            let latest = match report["text"].as_str() {
                Some(text) if readable => {
                    let sequence = &report["sequence"];
                    json!({"sequence":sequence,"kind":report["kind"],"source":report["source"],
                        "agentId":report["agentId"],"at":report["at"],
                        "text":spill(text, MAX_SUMMARY, || format!("ticket/{id}/report/{sequence}"))})
                }
                _ => Value::Null,
            };
            let mine = conversation
                .as_deref()
                .is_some_and(|conversation| ticket["conversationId"] == conversation);
            let waiting = if readable {
                ticket["waiting"].clone()
            } else {
                restricted_waiting(&ticket["waiting"])
            };
            let mut entry = json!({"id":id,"title":ticket["title"],"status":ticket["status"],
                "waiting":waiting,"thisConversation":mine,"agents":agents,
                "latestReport":latest,"updatedAt":ticket["updatedAt"],
                "read":format!("ticket/{id}")});
            if !readable {
                entry["restricted"] = json!(true);
            }
            entry
        })
        .collect()
}

/// Each of a ticket's `assignments` with its Agent's live status from `directory`, or `missing`
/// for an Agent the directory read does not have.
fn assigned(assignments: &[Value], directory: &HashMap<&str, &Value>, missing: &Value) -> Value {
    assignments
        .iter()
        .map(|assignment| {
            let id = &assignment["agentId"];
            match id.as_str().and_then(|id| directory.get(id)) {
                Some(agent) => json!({"id":id,"status":agent["status"],
                    "requiresAttention":agent["requiresAttention"]}),
                None => json!({"id":id,"status":missing}),
            }
        })
        .collect()
}

/// Project logged notification `entry` to its present fields, sequence, wake state and the
/// resource that reads it whole.
fn notification(entry: &Value) -> Value {
    let source = &entry["notification"];
    let read = format!("notification/{}", entry["sequence"]);
    let mut summary = json!({"sequence":entry["sequence"],"wake":entry["wake"]["state"]});
    for field in NOTIFICATION_FIELDS {
        if !source[field].is_null() {
            summary[field] = source[field].clone();
        }
    }
    if let Some(text) = source["summary"].as_str() {
        summary["summary"] = spill(text, MAX_SUMMARY, || read.clone());
    }
    summary["read"] = json!(read);
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
