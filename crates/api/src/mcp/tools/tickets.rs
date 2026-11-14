//! Bot tickets on the scoped MCP (ADR-274): the ticket every Agent a Bot starts joins, the Bot's
//! ticket tools and the worker's progress report.

use bot::mcp::CallerKind;
use bot::service::Bots;
use bot::tickets::{AssignTicket, OPEN_LIMIT_MESSAGE, OpenTicket, OpenedTicket, WorkerReport};
use domain::bot::BOT_TICKET_LABEL;
use domain::bot::ticket::{ASSIGNMENT_LIMIT, TicketDraft, TicketUpdate, WorkerReportKind};
use domain::{ConversationId, TicketId};
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;
use crate::mcp::Scope;
use crate::mcp::scope::Failure;

use super::{creation, recall, take};

/// Refusal for a `ticketId` that names no ticket of the calling Bot.
const UNKNOWN_TICKET: &str =
    "No such ticket; call bot_tickets to list your tickets, or omit ticketId to open a new one";
/// Refusal for starting an Agent on a closed ticket.
const CLOSED_TICKET: &str = "This ticket is closed; reopen it with bot_ticket_update \
    (status \"open\") before starting another Agent on it, or omit ticketId to open a new ticket";
/// Refusal for starting an Agent on a ticket that already has the most Agents.
const FULL_TICKET: &str = "This ticket already has the most Agents it can hold; give the work \
    to one of its Agents with send_agent_prompt, or omit ticketId to open a new ticket";
/// Refusal for closing a closed ticket with another status.
const CLOSED_CONFLICT: &str = "This ticket is already closed with another status; reopen it \
    with status \"open\" first if you need to change it";
/// Refusal for an update that changes nothing or pairs an outcome with a non-closing status.
const EMPTY_UPDATE: &str = "Give at least one of status, outcome, title or note; outcome is \
    accepted only together with status done, failed or cancelled";
/// Refusal for a report on a closed ticket or while the Bot is paused.
const REPORT_REFUSED: &str = "Your ticket is closed or the Workspace Bot is paused, so the report \
    was not recorded; finish your current work and reply normally";
/// Refusal for changing or joining a ticket another conversation originated (ADR-280).
const FOREIGN_TICKET: &str = "This ticket belongs to another conversation of the Workspace \
    Bot; only that conversation or the workspace owner's Ait conversation can change it";
/// Refusal for a report whose ticket the Bot no longer has.
const REPORT_GONE: &str = "Your ticket no longer exists, so the report was not recorded; finish \
    your current work and reply normally";

/// Whether `tool` is one of the ticket tools handled by [`execute`].
pub(super) fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "bot_tickets" | "bot_ticket_update" | "report_progress"
    )
}

/// Run ticket `tool` for `scope`'s caller; `context` is a subtree caller's own Agent record.
/// # Errors
/// Returns `Forbidden` without a scoped credential or for a caller the tool is not for, guided
/// refusals for unknown, closed or full tickets and refused reports, and Bot failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &str,
    arguments: Value,
    scope: Option<&Scope>,
    context: Option<&Value>,
) -> Result<Value, Failure> {
    let scope = scope.ok_or(ErrorCode::Forbidden)?;
    let root = scope.caller.root.as_str();
    match tool {
        "bot_tickets" => {
            let params = json!({"workspaceId":root,
                "includeClosed":arguments.get("includeClosed").unwrap_or(&json!(false))});
            let mut listed = rpc::execute(state, "bot.ticket.list.request", params).await?;
            recall::restrict_tickets(
                &mut listed,
                recall::sight(state, scope, context).await?.as_ref(),
            );
            Ok(listed)
        }
        "bot_ticket_update" => {
            originated(state, scope, &arguments["ticketId"]).await?;
            update(bots(state)?, root, arguments).await
        }
        _ => report(bots(state)?, scope, &arguments, context).await,
    }
}

/// Create an Agent for scoped `scope`, joining it to a ticket (ADR-274 §6).
///
/// A Bot session without `ticketId` opens a ticket keyed by the creation's idempotency key,
/// generating and passing one when the call has none; with `ticketId` it joins that open
/// ticket, if its conversation may read it (ADR-280 §4): a worker reads its whole ticket, so
/// joining another person's ticket would read it. A subtree caller on a ticket passes it on to
/// the child. The Agent carries
/// `ait.bot.ticket` and is then assigned; a failed assignment is logged and does not fail the
/// creation, since the Agent already runs and its first report assigns it again.
/// `arguments` are already confined; `context` is a subtree caller's own Agent record.
/// # Errors
/// Returns the open-ticket limit, unknown, foreign, closed or full ticket refusals before
/// anything is created, and creation failures, after deleting a ticket this call opened for
/// the Agent.
pub(super) async fn create_agent(
    state: &Shared,
    scope: &Scope,
    mut arguments: Value,
    context: Option<&Value>,
) -> Result<Value, Failure> {
    let requested = take(&mut arguments, "ticketId")?;
    let joined = match (&scope.caller.kind, requested) {
        (CallerKind::Bot { .. }, Some(ticket)) => {
            Some(Join::Named(named(state, scope, &ticket).await?))
        }
        (
            CallerKind::Bot {
                conversation_id, ..
            },
            None,
        ) => Some(Join::Opened(
            open(state, scope, *conversation_id, &mut arguments).await?,
        )),
        (CallerKind::Subtree { .. }, Some(_)) => return Err(ErrorCode::Forbidden.into()),
        (CallerKind::Subtree { ticket, .. }, None) => ticket.map(Join::Inherited),
    };
    let Some(joined) = joined else {
        return Ok(creation::agent(state, arguments, context).await?);
    };
    let ticket = joined.ticket();
    let workspace = arguments["workspaceId"].clone();
    arguments["labels"][BOT_TICKET_LABEL] = json!(ticket);
    let created = creation::agent(state, arguments, context).await;
    let agent = created
        .as_ref()
        .ok()
        .and_then(|result| result["agent"]["id"].as_str());
    let Some(agent) = agent else {
        if let Join::Opened(OpenedTicket { created: true, .. }) = joined {
            abandon(bots(state)?, &scope.caller.root, ticket).await;
        }
        return Ok(created?);
    };
    let assignment = AssignTicket {
        root: scope.caller.root.clone(),
        ticket_id: ticket,
        agent_id: agent.to_owned(),
        workspace_id: workspace.as_str().unwrap_or_default().to_owned(),
    };
    if let Err(error) = bots(state)?.assign_ticket(assignment).await {
        tracing::warn!(?error, %ticket, "Agent was created without its ticket assignment");
    }
    let mut created = created?;
    created["ticketId"] = json!(ticket);
    Ok(created)
}

/// How a new Agent joins a ticket.
#[derive(Debug, Clone, Copy)]
enum Join {
    /// A ticket this creation opened, or an earlier attempt with the same key opened.
    Opened(OpenedTicket),
    /// An open ticket the Bot named.
    Named(TicketId),
    /// The ticket of the subtree caller.
    Inherited(TicketId),
}

impl Join {
    const fn ticket(self) -> TicketId {
        match self {
            Self::Opened(opened) => opened.ticket_id,
            Self::Named(ticket) | Self::Inherited(ticket) => ticket,
        }
    }
}

fn bots(state: &Shared) -> Result<&Bots, ErrorCode> {
    state.bot.bots.as_ref().ok_or(ErrorCode::BotNotFound)
}

/// Open the ticket of a Bot session's new Agent, titled and asked as the Agent is, keyed by the
/// creation's idempotency key (written into `arguments` when absent).
///
/// The ticket records the session's `conversation`; a session whose conversation the Bot no
/// longer has opens it without one rather than failing. At the open ticket limit, the tickets
/// whose work is evidently over are closed first and the opening is retried once.
/// # Errors
/// Returns the open-ticket limit refusal and other Bot failures.
async fn open(
    state: &Shared,
    scope: &Scope,
    conversation: ConversationId,
    arguments: &mut Value,
) -> Result<OpenedTicket, Failure> {
    if arguments.get("idempotencyKey").is_none() {
        arguments["idempotencyKey"] = json!(uuid::Uuid::new_v4().to_string());
    }
    let key = arguments["idempotencyKey"]
        .as_str()
        .ok_or(ErrorCode::InvalidMessage)?
        .to_owned();
    let bots = bots(state)?;
    let mut request = OpenTicket {
        root: scope.caller.root.clone(),
        draft: TicketDraft {
            conversation_id: Some(conversation),
            key,
            title: arguments["title"].as_str().unwrap_or_default().to_owned(),
            request: arguments["initialPrompt"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        },
    };
    let opened = match bots.open_ticket(request.clone()).await {
        Err(ErrorCode::InvalidMessage) => {
            request.draft.conversation_id = None;
            bots.open_ticket(request.clone()).await
        }
        other => other,
    };
    let opened = match opened {
        Err(ErrorCode::ResourceExhausted) if close_finished(state, bots, &request.root).await => {
            bots.open_ticket(request).await
        }
        other => other,
    };
    match opened {
        Err(ErrorCode::ResourceExhausted) => Err(Failure::Refused(OPEN_LIMIT_MESSAGE)),
        other => Ok(other?),
    }
}

/// Close `root`'s open or waiting tickets whose work is evidently over: tickets with at least
/// one Agent, none of which is still unarchived according to a complete read of the Agent
/// directory; a waiting ticket's block or permission request then has nobody left to answer it.
/// Runs only at the open ticket limit; a read that fails or is incomplete closes nothing.
/// Returns whether any ticket was closed.
async fn close_finished(state: &Shared, bots: &Bots, root: &str) -> bool {
    let listed = rpc::execute(
        state,
        "bot.ticket.list.request",
        json!({"workspaceId":root}),
    )
    .await;
    let Ok(listed) = listed else {
        return false;
    };
    let mut closed = false;
    let candidates = listed["tickets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|ticket| {
            matches!(ticket["status"].as_str(), Some("open" | "waiting"))
                && ticket["assignments"]
                    .as_array()
                    .is_some_and(|assignments| !assignments.is_empty())
        });
    for ticket in candidates {
        let Some(id) = ticket["id"]
            .as_str()
            .and_then(|id| id.parse::<TicketId>().ok())
        else {
            continue;
        };
        if !agents_archived(state, id).await {
            continue;
        }
        match bots.close_finished_ticket(root.to_owned(), id).await {
            Ok(status) => closed |= status.is_some(),
            Err(error) => tracing::warn!(?error, %id, "finished ticket was not closed"),
        }
    }
    closed
}

/// Whether no unarchived Agent carries `ticket`'s label, from a complete directory read.
async fn agents_archived(state: &Shared, ticket: TicketId) -> bool {
    let page = rpc::execute(
        state,
        "agent.list.request",
        json!({"filter":{"labels":{BOT_TICKET_LABEL:ticket}},"page":{"limit":1}}),
    )
    .await;
    page.is_ok_and(|page| {
        page["entries"].as_array().is_some_and(Vec::is_empty)
            && page["pageInfo"]["nextCursor"].is_null()
    })
}

/// Require `ticket` to name an open ticket of the scope's Bot that the calling conversation
/// may read, with room for another Agent.
/// # Errors
/// Returns the unknown, foreign, closed or full ticket refusals and other Bot failures.
async fn named(state: &Shared, scope: &Scope, ticket: &Value) -> Result<TicketId, Failure> {
    let id: TicketId = ticket
        .as_str()
        .and_then(|ticket| ticket.parse().ok())
        .ok_or(Failure::Refused(UNKNOWN_TICKET))?;
    let found = rpc::execute(
        state,
        "bot.ticket.get.request",
        json!({"workspaceId":scope.caller.root,"ticketId":id}),
    )
    .await;
    let found = match found {
        Err(ErrorCode::BotNotFound) => return Err(Failure::Refused(UNKNOWN_TICKET)),
        other => other?,
    };
    let ticket = &found["ticket"];
    let sight = recall::sight(state, scope, None).await?;
    if !recall::sees(sight.as_ref(), &ticket["conversationId"]) {
        return Err(Failure::Refused(FOREIGN_TICKET));
    }
    if !matches!(ticket["status"].as_str(), Some("open" | "waiting")) {
        return Err(Failure::Refused(CLOSED_TICKET));
    }
    if ticket["assignments"]
        .as_array()
        .is_some_and(|assignments| assignments.len() >= ASSIGNMENT_LIMIT)
    {
        return Err(Failure::Refused(FULL_TICKET));
    }
    Ok(id)
}

/// Refuse a Bot conversation's change to `ticket` when another conversation originated it and
/// the caller is not the owner's conversation; an unknown ticket is left to the update, which
/// refuses it.
/// # Errors
/// Returns the foreign ticket refusal and Bot failures.
async fn originated(state: &Shared, scope: &Scope, ticket: &Value) -> Result<(), Failure> {
    let sight = recall::sight(state, scope, None).await?;
    if !recall::external(sight.as_ref()) {
        return Ok(());
    }
    let found = rpc::execute(
        state,
        "bot.ticket.get.request",
        json!({"workspaceId":scope.caller.root,"ticketId":ticket}),
    )
    .await;
    match found {
        Ok(found) if !recall::sees(sight.as_ref(), &found["ticket"]["conversationId"]) => {
            Err(Failure::Refused(FOREIGN_TICKET))
        }
        _ => Ok(()),
    }
}

/// Delete the ticket this call opened for an Agent that could not be created; a failure only
/// leaves an open ticket without Agents, which the Bot or the user can close.
async fn abandon(bots: &Bots, root: &str, ticket: TicketId) {
    if let Err(error) = bots.abandon_ticket(root.to_owned(), ticket).await {
        tracing::warn!(?error, %ticket, "ticket of an Agent that was not created was kept");
    }
}

/// `bot_ticket_update`: the Bot closes, reopens, retitles or annotates one of its tickets.
/// # Errors
/// Returns guided refusals for unknown tickets, conflicting closes, empty updates and the open
/// ticket limit on reopening, and other Bot failures.
async fn update(bots: &Bots, root: &str, mut arguments: Value) -> Result<Value, Failure> {
    let ticket: TicketId = take(&mut arguments, "ticketId")?
        .as_ref()
        .and_then(Value::as_str)
        .and_then(|ticket| ticket.parse().ok())
        .ok_or(Failure::Refused(UNKNOWN_TICKET))?;
    let update: TicketUpdate =
        serde_json::from_value(arguments).map_err(|_| Failure::Refused(EMPTY_UPDATE))?;
    match bots.update_ticket(root.to_owned(), ticket, update).await {
        Ok(result) => Ok(result),
        Err(ErrorCode::BotNotFound) => Err(Failure::Refused(UNKNOWN_TICKET)),
        Err(ErrorCode::BotConflict) => Err(Failure::Refused(CLOSED_CONFLICT)),
        Err(ErrorCode::InvalidMessage) => Err(Failure::Refused(EMPTY_UPDATE)),
        Err(ErrorCode::ResourceExhausted) => Err(Failure::Refused(OPEN_LIMIT_MESSAGE)),
        Err(error) => Err(error.into()),
    }
}

/// `report_progress`: a subtree Agent reports on the ticket its own label names.
///
/// The label is daemon-written, so it authorizes the report as it did the creation; an Agent
/// not yet assigned, because its assignment failed or the ticket was full, is assigned while
/// the ticket has room and heard either way.
/// # Errors
/// Returns `Forbidden` for callers without a ticket and guided refusals when the report cannot
/// be recorded.
async fn report(
    bots: &Bots,
    scope: &Scope,
    arguments: &Value,
    context: Option<&Value>,
) -> Result<Value, Failure> {
    let CallerKind::Subtree {
        agent_id,
        ticket: Some(ticket),
    } = &scope.caller.kind
    else {
        return Err(ErrorCode::Forbidden.into());
    };
    let kind: WorkerReportKind =
        serde_json::from_value(arguments["kind"].clone()).map_err(|_| ErrorCode::InvalidMessage)?;
    let report = WorkerReport {
        root: scope.caller.root.clone(),
        ticket_id: *ticket,
        agent_id: agent_id.clone(),
        workspace_id: context
            .and_then(|agent| agent["workspaceId"].as_str())
            .unwrap_or(&scope.caller.root)
            .to_owned(),
        kind,
        text: arguments["text"].as_str().unwrap_or_default().to_owned(),
    };
    match bots.report_progress(report).await {
        Ok(status) => Ok(json!({"ticketId":ticket,"ticketStatus":status})),
        Err(ErrorCode::BotConflict) => Err(Failure::Refused(REPORT_REFUSED)),
        Err(ErrorCode::BotNotFound) => Err(Failure::Refused(REPORT_GONE)),
        Err(error) => Err(error.into()),
    }
}

/// Before a Bot session's `send_agent_prompt` reaches confined Agent `arguments.agentId`,
/// reopen that Agent's closed ticket, so its reports on the new work are recorded. At the open
/// ticket limit the tickets whose work is evidently over are closed first and the reopen is
/// retried once. Best effort: the prompt goes through either way, and a ticket reopened for a
/// prompt that then fails stays open. Returns a note for the Bot when the ticket stays closed
/// because of the limit.
pub(super) async fn reopen_for_prompt(
    state: &Shared,
    scope: &Scope,
    arguments: &Value,
) -> Option<String> {
    let (CallerKind::Bot { .. }, Ok(bots)) = (&scope.caller.kind, bots(state)) else {
        return None;
    };
    let agent = arguments["agentId"].as_str()?;
    let record = rpc::execute(state, "agent.get.request", json!({"agentId":agent})).await;
    let ticket = record.ok().and_then(|record| {
        record["agent"]["labels"][BOT_TICKET_LABEL]
            .as_str()
            .and_then(|ticket| ticket.parse::<TicketId>().ok())
    })?;
    let root = &scope.caller.root;
    let reopen = || bots.reopen_ticket(root.clone(), ticket, agent.to_owned());
    let reopened = match reopen().await {
        Err(ErrorCode::ResourceExhausted) if close_finished(state, bots, root).await => {
            reopen().await
        }
        other => other,
    };
    match reopened {
        Ok(_) | Err(ErrorCode::BotNotFound) => None,
        Err(ErrorCode::ResourceExhausted) => Some(format!(
            "Ticket {ticket} of this Agent is closed and was not reopened because you reached \
             your open ticket limit, so the Agent's reports on this work will be refused; close \
             tickets whose work is finished or no longer needed with bot_ticket_update (status \
             done or cancelled), then reopen {ticket} with status \"open\""
        )),
        Err(error) => {
            tracing::warn!(?error, %ticket, "ticket of a prompted Agent stays closed");
            None
        }
    }
}

#[cfg(test)]
mod tests;
