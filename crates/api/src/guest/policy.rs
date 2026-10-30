//! Default-deny request policy for guest connections.
//!
//! Each admitted method is checked against the guest's single workspace, rewritten where
//! the workspace must be forced, or executed privately when its reply must be filtered.
//! Refusals use [`ErrorCode::Forbidden`] and never reveal other workspaces.

mod agents;
pub(crate) mod private;

use model::{ErrorCode, Request};
use serde_json::{Map, Value, json};

use super::{Session, conceal};
use crate::Shared;

use private::Private;

/// What the connection does with a guest request.
#[derive(Debug)]
pub(crate) enum Decision {
    /// Dispatch normally with these, possibly rewritten, parameters.
    Forward(Request),
    /// Dispatch once without subscriptions and post-process the reply for this guest.
    Private(Request, Private),
    /// Refuse the request with `code`.
    Refuse(Request, ErrorCode),
}

/// What the connection does with a guest event.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EventDecision {
    /// Handle the event normally.
    Forward,
    /// Ignore the event without replying.
    Drop,
    /// Reply with this error.
    Refuse(ErrorCode),
}

/// How one method is confined to the guest's workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// Reveals nothing about other workspaces.
    Open,
    /// Provider catalog read with an optional `cwd`.
    Catalog,
    /// `workspaceId` must name only the guest's workspace.
    Workspace,
    /// Listed workspaces are filtered to the guest's workspace.
    WorkspaceList,
    /// Agent creation is forced into the workspace and labelled as guest-created.
    AgentCreate,
    /// The Agent named by this parameter must belong to the workspace; read-only.
    Agent(&'static str),
    /// The Agent named by this parameter must have been created by a guest of this
    /// share. Owner Agents keep the daemon MCP, which reaches every workspace, so
    /// guests may read them but never instruct them.
    AgentDrive(&'static str),
    /// As [`Rule::Agent`], without changing labels.
    AgentUpdate,
    /// As [`Rule::Agent`], without draft configurations.
    AgentCommands,
    /// Directory filters are forced to the workspace.
    AgentFilter,
    /// Every listed Agent must belong to the workspace.
    AgentSubscription,
    /// Every listed Agent and terminal must belong to the workspace.
    ItemsClose,
    /// Terminal creation is forced into the workspace.
    TerminalCreate,
    /// Terminal listings are forced to the workspace.
    TerminalFilter,
    /// The terminal named by `terminalId` must belong to the workspace.
    Terminal,
    /// `cwd` must resolve inside the workspace root.
    Path,
    /// As [`Rule::Path`], within the guest's download token budget.
    Download,
    /// Not available to guests.
    Deny,
}

fn classify(method: &str) -> Rule {
    match method {
        "server.info"
        | "connection.ping"
        | "subscription.release.request"
        | "server.status.subscribe"
        | "server.status.unsubscribe"
        | "fs.file.unsubscribe.request"
        | "checkout.diff.unsubscribe.request" => Rule::Open,
        "provider.available.list.request"
        | "provider.models.list.request"
        | "provider.modes.list.request"
        | "provider.features.list.request"
        | "provider.snapshot.get.request" => Rule::Catalog,
        "workspace.get.request"
        | "workspace.setup.status.request"
        | "workspace.setup.summary.request"
        | "workspace.setup.subscribe.request"
        | "workspace.script.list.request"
        | "workspace.script.start.request"
        | "workspace.script.stop.request"
        | "workspace.clear_attention.request"
        | "workspace.mark_unread.request" => Rule::Workspace,
        "workspace.list.request" => Rule::WorkspaceList,
        "agent.create.request" => Rule::AgentCreate,
        "agent.get.request"
        | "agent.finish.wait.request"
        | "agent.timeline.get.request"
        | "agent.timeline.search.request"
        | "agent.timeline.list_prompts.request" => Rule::Agent("agentId"),
        "agent.archive.request"
        | "agent.attention.clear.request"
        | "agent.message.send.request"
        | "agent.cancel.request"
        | "agent.model.set.request"
        | "agent.thinking.set.request"
        | "agent.mode.set.request"
        | "agent.feature.set.request"
        | "agent.permission.resolve.request"
        | "agent.rewind.request"
        | "agent.context.compact.request" => Rule::AgentDrive("agentId"),
        "agent.provider_subagents.list.request"
        | "agent.provider_subagents.timeline.get.request" => Rule::Agent("parentAgentId"),
        "agent.update.request" => Rule::AgentUpdate,
        "agent.commands.list.request" => Rule::AgentCommands,
        "agent.list.request" | "agent.history.get.request" => Rule::AgentFilter,
        "agent.timeline.set_subscription.request" => Rule::AgentSubscription,
        "agent.items.close.request" => Rule::ItemsClose,
        "terminal.create.request" => Rule::TerminalCreate,
        "terminal.list.request"
        | "terminal.list.subscribe.request"
        | "terminal.list.unsubscribe.request" => Rule::TerminalFilter,
        "terminal.rename.request"
        | "terminal.subscribe.request"
        | "terminal.unsubscribe.request"
        | "terminal.kill.request"
        | "terminal.capture.request" => Rule::Terminal,
        "fs.file.download_token.request" => Rule::Download,
        "directory.suggestions.request"
        | "fs.explorer.request"
        | "fs.file.subscribe.request"
        | "fs.file.write.request"
        | "fs.entry.create.request"
        | "fs.entry.rename.request"
        | "fs.entry.duplicate.request"
        | "fs.entry.delete.request"
        | "forge.search.request"
        | "github.search.request" => Rule::Path,
        method if method.starts_with("checkout.") => Rule::Path,
        _ => Rule::Deny,
    }
}

/// Confine one guest request to the guest's workspace.
pub(crate) async fn request(session: &Session, state: &Shared, mut request: Request) -> Decision {
    let rule = classify(&request.method);
    let params = std::mem::take(&mut request.params);
    match confine(session, state, rule, params).await {
        Ok(Confined::Forward(params)) => Decision::Forward(Request { params, ..request }),
        Ok(Confined::Private(params, private)) => {
            Decision::Private(Request { params, ..request }, private)
        }
        Err(code) => Decision::Refuse(request, code),
    }
}

enum Confined {
    Forward(Value),
    Private(Value, Private),
}

async fn confine(
    session: &Session,
    state: &Shared,
    rule: Rule,
    mut params: Value,
) -> Result<Confined, ErrorCode> {
    match rule {
        Rule::Open => {}
        Rule::Deny => return Err(ErrorCode::Forbidden),
        Rule::Catalog => optional_path(session, &params).await?,
        Rule::Workspace => only_workspace(session, &params["workspaceId"])?,
        Rule::WorkspaceList => {
            return private::workspace_list(params)
                .map(|params| Confined::Private(params, Private::WorkspaceList));
        }
        Rule::TerminalCreate => {
            force_terminal(session, &mut params, false).await?;
            return Ok(Confined::Private(params, Private::TerminalCreate));
        }
        Rule::TerminalFilter => force_terminal(session, &mut params, true).await?,
        Rule::Terminal => {
            let id = params["terminalId"].as_str().ok_or(ErrorCode::Forbidden)?;
            terminal(session, state, id).await?;
        }
        Rule::Path => path(session, &params).await?,
        Rule::Download => {
            path(session, &params).await?;
            if !session.take_download(std::time::Instant::now()) {
                return Err(ErrorCode::ResourceExhausted);
            }
        }
        Rule::AgentCreate
        | Rule::Agent(_)
        | Rule::AgentDrive(_)
        | Rule::AgentUpdate
        | Rule::AgentCommands
        | Rule::AgentFilter
        | Rule::AgentSubscription
        | Rule::ItemsClose => agents::confine(session, state, rule, &mut params).await?,
    }
    Ok(Confined::Forward(params))
}

/// Confine one guest event; heartbeats are dropped because a guest is not the owner's client.
pub(crate) async fn event(
    session: &Session,
    state: &Shared,
    method: &str,
    params: &Value,
) -> EventDecision {
    match method {
        "session.heartbeat" => EventDecision::Drop,
        "terminal.input" => match params["terminalId"].as_str() {
            Some(id) => match terminal(session, state, id).await {
                Ok(()) => EventDecision::Forward,
                Err(code) => EventDecision::Refuse(code),
            },
            None => EventDecision::Refuse(ErrorCode::Forbidden),
        },
        _ => EventDecision::Refuse(ErrorCode::Forbidden),
    }
}

fn object(params: &mut Value) -> Result<&mut Map<String, Value>, ErrorCode> {
    if params.is_null() {
        *params = json!({});
    }
    params.as_object_mut().ok_or(ErrorCode::InvalidMessage)
}

fn present(value: Option<&Value>) -> bool {
    value.is_some_and(|value| !value.is_null())
}

fn only_workspace(session: &Session, selection: &Value) -> Result<(), ErrorCode> {
    let workspace = session.workspace_id();
    let only = match selection {
        Value::String(id) => id == workspace,
        Value::Array(ids) => !ids.is_empty() && ids.iter().all(|id| id == workspace),
        _ => false,
    };
    only.then_some(()).ok_or(ErrorCode::Forbidden)
}

async fn path(session: &Session, params: &Value) -> Result<(), ErrorCode> {
    let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
    if session.contains(cwd).await {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

async fn optional_path(session: &Session, params: &Value) -> Result<(), ErrorCode> {
    if present(params.get("cwd")) {
        path(session, params).await
    } else {
        Ok(())
    }
}

/// Force `workspaceId` and a `cwd` inside the workspace; listings keep a contained `cwd`.
async fn force_terminal(
    session: &Session,
    params: &mut Value,
    listing: bool,
) -> Result<(), ErrorCode> {
    if listing {
        optional_path(session, params).await?;
    } else if present(params.get("cwd")) {
        path(session, params).await?;
    }
    let object = object(params)?;
    object.insert("workspaceId".to_owned(), json!(session.workspace_id()));
    if !present(object.get("cwd")) {
        object.insert("cwd".to_owned(), json!(session.root_text));
    }
    Ok(())
}

/// Require terminal `id` to belong to the guest's workspace.
async fn terminal(session: &Session, state: &Shared, id: &str) -> Result<(), ErrorCode> {
    if session.knows_terminal(id) {
        return Ok(());
    }
    let listed = crate::connection::rpc::execute(
        state,
        "terminal.list.request",
        json!({"workspaceId": session.workspace_id()}),
    )
    .await
    .map_err(conceal)?;
    let found = listed["terminals"]
        .as_array()
        .is_some_and(|terminals| terminals.iter().any(|terminal| terminal["id"] == id));
    if !found {
        return Err(ErrorCode::Forbidden);
    }
    session.remember_terminal(id);
    Ok(())
}

#[cfg(test)]
mod tests;
