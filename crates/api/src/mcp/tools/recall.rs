//! Reading the Workspace Bot's records by id on the scoped MCP (ADR-280): the Bot's `bot_read`,
//! a worker's `ticket_read`, and what a Bot conversation may read of the other tools' results.

use bot::mcp::CallerKind;
use bot::recall::{Origin, Read, Reader, Sight, Viewer};
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;
use crate::mcp::Scope;
use crate::mcp::scope::Failure;

/// Refusal for an unknown `bot_read` resource or page size.
const UNKNOWN_RESOURCE: &str = "Unknown resource or page size. Resources are tickets, \
    tickets/closed, ticket/<id>, ticket/<id>/reports, ticket/<id>/report/<n>, \
    ticket/<id>/request, ticket/<id>/outcome, conversations, conversation/<id>, turn/<id>, \
    turn/<id>/input, turn/<id>/message/<n>, notifications, notification/<sequence> and \
    agent/<id>; limit is at most 50 for lists and 8000 for text";
/// Refusal for an unknown `ticket_read` resource or page size.
const UNKNOWN_WORKER_RESOURCE: &str = "Unknown resource or page size. Resources are ticket, \
    ticket/reports, ticket/report/<n>, ticket/request and ticket/outcome; limit is at most 50 \
    for lists and 8000 for text";
/// Refusal for a record that is not kept.
const NOT_KEPT: &str = "Nothing is kept under this resource: the id is unknown or the record \
    was dropped. List tickets, conversations or notifications to find current ids";
/// Refusal for a worker's read of a ticket or report that is not kept.
const NOT_KEPT_WORKER: &str = "Nothing is kept under this resource: your ticket is no longer \
    kept, or it has no such report. Read ticket/reports for the reports that are kept, or finish \
    your current work and reply normally";
/// Refusal for an Agent an external conversation may not reach, and for an id that names no
/// Agent, so a refusal does not confirm that an Agent exists (ADR-280 §4).
pub(in crate::mcp) const UNREACHABLE_AGENT: &str = "No Agent this conversation may reach matches \
    this id: it is unknown, or it works for another conversation of the Workspace Bot or for \
    the workspace owner. Use the Agents of this conversation's own tickets or list_agents; only \
    the workspace owner's Ait conversation reaches every Agent";
/// Refusal for a record another conversation originated.
const OTHER_CONVERSATION: &str = "This belongs to another conversation of the Workspace Bot, \
    which this conversation may not read; only the workspace owner's Ait conversation reads \
    every conversation";

/// Whether `tool` is one of the read tools handled by [`execute`].
pub(super) fn handles(tool: &str) -> bool {
    matches!(tool, "bot_read" | "ticket_read")
}

/// Run read `tool` for `scope`'s caller: `bot_read` for a Bot session, `ticket_read` for a
/// worker, whose resources always name its own ticket.
/// # Errors
/// Returns `Forbidden` for any other caller, guided refusals for unknown resources, records
/// that are not kept and records of another conversation, and Bot failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &str,
    arguments: &Value,
    scope: Option<&Scope>,
) -> Result<Value, Failure> {
    let scope = scope.ok_or(ErrorCode::Forbidden)?;
    let (reader, unknown, not_kept) = match (&scope.caller.kind, tool) {
        (
            CallerKind::Bot {
                conversation_id, ..
            },
            "bot_read",
        ) => (
            Reader::Conversation(*conversation_id),
            UNKNOWN_RESOURCE,
            NOT_KEPT,
        ),
        (
            CallerKind::Subtree {
                ticket: Some(ticket),
                ..
            },
            "ticket_read",
        ) => (
            Reader::Worker(*ticket),
            UNKNOWN_WORKER_RESOURCE,
            NOT_KEPT_WORKER,
        ),
        _ => return Err(ErrorCode::Forbidden.into()),
    };
    let bots = state.bot.bots.as_ref().ok_or(ErrorCode::BotNotFound)?;
    let resource = arguments["resource"]
        .as_str()
        .unwrap_or("ticket")
        .to_owned();
    let (agent, other) = match (reader, resource.strip_prefix("agent/")) {
        (Reader::Conversation(_), Some(id)) => (agent(state, scope, id).await?, UNREACHABLE_AGENT),
        _ => (None, OTHER_CONVERSATION),
    };
    let read = Read {
        root: scope.caller.root.clone(),
        reader,
        resource,
        offset: count(&arguments["offset"])?,
        limit: count(&arguments["limit"])?,
        agent,
    };
    let mut page = match bots.read(read).await {
        Ok(page) => page,
        Err(ErrorCode::InvalidMessage) => return Err(Failure::Refused(unknown)),
        Err(ErrorCode::BotNotFound) => return Err(Failure::Refused(not_kept)),
        Err(ErrorCode::Forbidden) => return Err(Failure::Refused(other)),
        Err(error) => return Err(error.into()),
    };
    join_live(state, &mut page).await;
    Ok(page)
}

/// A non-negative integer argument, or `None` when absent.
fn count(value: &Value) -> Result<Option<usize>, ErrorCode> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .and_then(|count| usize::try_from(count).ok())
        .map(Some)
        .ok_or(ErrorCode::InvalidMessage)
}

/// The public record of Agent `id` when it runs in the caller's scope; `None` otherwise, which
/// reads as not kept for the owner and as unreachable for an external conversation.
/// # Errors
/// Propagates Agent directory failures.
async fn agent(state: &Shared, scope: &Scope, id: &str) -> Result<Option<Value>, ErrorCode> {
    let mut found = rpc::execute(
        state,
        "agent.get.request",
        json!({"agentId":id,"publicOnly":true}),
    )
    .await?;
    let record = found["agent"].take();
    let in_scope = record["workspaceId"]
        .as_str()
        .is_some_and(|workspace| scope.workspaces().contains(workspace));
    Ok(in_scope.then_some(record))
}

/// Join every assignment of a ticket page with its Agent's live title and status from the
/// Agent directory; an Agent the directory no longer has reads as archived.
async fn join_live(state: &Shared, page: &mut Value) {
    let Some(assignments) = page["assignments"].as_array_mut() else {
        return;
    };
    for assignment in assignments {
        let Some(id) = assignment["agentId"].as_str() else {
            continue;
        };
        let found = rpc::execute(
            state,
            "agent.get.request",
            json!({"agentId":id,"publicOnly":true}),
        )
        .await;
        let record = found.map(|mut found| found["agent"].take());
        let live = match record {
            Ok(record) if record.is_object() => json!({"title":record["title"],
                "status":record["status"],"requiresAttention":record["requiresAttention"],
                "archived":record["archivedAt"].is_string()}),
            Ok(_) => json!({"status":null,"archived":true}),
            Err(error) => {
                tracing::debug!(?error, "live Agent state of a ticket was not read");
                json!({"status":null})
            }
        };
        if let (Some(assignment), Some(live)) = (assignment.as_object_mut(), live.as_object()) {
            assignment.extend(live.clone());
        }
    }
}

/// What the caller may read and reach (ADR-280 §4): a Bot session the sight of its
/// conversation, a subtree caller that of the conversation its work came from, which is its
/// ticket's origin when the Bot keeps that ticket with one, otherwise the conversation label of
/// its own Agent record `context`. Its own children carry the same ticket and label, so it
/// reaches them. `None` for a subtree caller whose work came from no conversation and once the
/// Bot is gone, which the callers treat as everything their tool already allows.
/// # Errors
/// Propagates other Bot failures.
pub(in crate::mcp) async fn sight(
    state: &Shared,
    scope: &Scope,
    context: Option<&Value>,
) -> Result<Option<Sight>, ErrorCode> {
    let Some(bots) = &state.bot.bots else {
        return Ok(None);
    };
    let origin = match &scope.caller.kind {
        CallerKind::Bot {
            conversation_id, ..
        } => Origin::Conversation(*conversation_id),
        CallerKind::Subtree { ticket, .. } => Origin::Agent {
            ticket: *ticket,
            conversation: scope.origin(context),
        },
    };
    match bots.sight(scope.caller.root.clone(), origin).await {
        Err(ErrorCode::BotNotFound) => Ok(None),
        other => other,
    }
}

/// Whether `sight` reaches only its own work: an external conversation's, or a subtree
/// caller's whose work came from one.
pub(in crate::mcp) fn external(sight: Option<&Sight>) -> bool {
    matches!(sight.map(|sight| sight.viewer), Some(Viewer::External(_)))
}

/// Whether `sight` may read what the conversation `origin` (a JSON id or `null`) originated;
/// without a sight everything is readable.
pub(super) fn sees(sight: Option<&Sight>, origin: &Value) -> bool {
    sight.is_none_or(|sight| {
        sight
            .viewer
            .sees(origin.as_str().and_then(|id| id.parse().ok()))
    })
}

/// Whether `sight` may see, direct or stop the Agent of directory record `agent`; without a
/// sight every Agent in scope is reachable.
pub(in crate::mcp) fn sees_agent(sight: Option<&Sight>, agent: &Value) -> bool {
    sight.is_none_or(|sight| sight.sees_agent(agent))
}

/// The `waiting` of a ticket a viewer may not read: why and since when it waits, without the
/// Agent whose event made it wait.
pub(super) fn restricted_waiting(waiting: &Value) -> Value {
    if waiting.is_object() {
        json!({"reason":waiting["reason"],"since":waiting["since"]})
    } else {
        Value::Null
    }
}

/// Reduce the tickets of a `bot.ticket.list.request` result that `sight` may not read to the
/// title and status the overview shows.
pub(super) fn restrict_tickets(listed: &mut Value, sight: Option<&Sight>) {
    for ticket in listed["tickets"].as_array_mut().into_iter().flatten() {
        if !sees(sight, &ticket["conversationId"]) {
            *ticket = json!({"id":ticket["id"],"title":ticket["title"],
                "status":ticket["status"],"waiting":restricted_waiting(&ticket["waiting"]),
                "updatedAt":ticket["updatedAt"],"closedAt":ticket["closedAt"],
                "restricted":true});
        }
    }
}

#[cfg(test)]
mod tests;
