//! Default-deny request policy for guest connections.
//!
//! Each admitted method is checked against the guest's shared workspace, project or node,
//! rewritten where placement must be forced, or executed privately when its reply must be
//! filtered. Refusals use [`ErrorCode::Forbidden`] and never reveal resources outside the
//! shared scope.

mod agents;
mod directory;
mod node;
pub(crate) mod private;
pub(crate) mod terminals;

use model::share::ResourceKind;
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

/// How one method is confined to the guest's shared scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// Reveals nothing outside the scope.
    Open,
    /// Provider catalog read with an optional `cwd`.
    Catalog,
    /// `workspaceId` must name only workspaces inside the scope.
    Workspace,
    /// As [`Rule::Workspace`] for renaming or archiving; project and node shares only.
    WorkspaceManage,
    /// Listed workspaces are filtered to the scope.
    WorkspaceList,
    /// Workspace creation is forced into the shared project; project and node shares only.
    WorkspaceCreate,
    /// Worktree creation is forced into the shared project; project and node shares only.
    WorktreeCreate,
    /// Worktree archive names only workspaces and paths in scope; project and node shares only.
    WorktreeArchive,
    /// Opening reuses a workspace of the shared project; project and node shares only.
    WorkspaceOpen,
    /// Listed projects are filtered to the shared project; project and node shares only.
    ProjectList,
    /// `projectId` must name the shared project; project and node shares only.
    Project,
    /// Every listed path parameter that is present must be in scope and one must be present;
    /// project and node shares only.
    Paths(&'static [&'static str]),
    /// Host-wide project and workspace management; node shares only.
    Node,
    /// Agent creation is forced into the scope and labelled as guest-created.
    AgentCreate,
    /// The Agent named by this parameter must belong to the scope; read-only.
    Agent(&'static str),
    /// The Agent named by this parameter must have been created by a guest of this
    /// share. Owner Agents keep the daemon MCP, which reaches every workspace, so
    /// guests may read them but never instruct them.
    AgentDrive(&'static str),
    /// As [`Rule::AgentDrive`], without changing labels.
    AgentUpdate,
    /// As [`Rule::Agent`], without draft configurations.
    AgentCommands,
    /// Directory filters are forced to the scope.
    AgentFilter,
    /// Every listed Agent must belong to the scope.
    AgentSubscription,
    /// Every listed Agent and terminal must belong to the scope.
    ItemsClose,
    /// Terminal creation is forced into a workspace in scope.
    TerminalCreate,
    /// Terminal listings are forced to a workspace in scope.
    TerminalFilter,
    /// The terminal named by `terminalId` must belong to the scope. A subscription to a
    /// terminal the visitor did not create cannot claim its size.
    Terminal,
    /// The terminal named by `terminalId` must have been created by this visitor.
    TerminalDrive,
    /// `cwd` must resolve inside the scope.
    Path,
    /// As [`Rule::Path`], within the guest's download token budget.
    Download,
    /// Not available to guests.
    Deny,
}

impl Rule {
    /// Whether a share of `kind` may use methods of this rule at all.
    fn admits(self, kind: ResourceKind) -> bool {
        match self {
            Self::Deny => false,
            Self::Node => kind == ResourceKind::Node,
            Self::WorkspaceManage
            | Self::WorkspaceCreate
            | Self::WorktreeCreate
            | Self::WorktreeArchive
            | Self::WorkspaceOpen
            | Self::ProjectList
            | Self::Project
            | Self::Paths(_) => kind != ResourceKind::Workspace,
            Self::Open
            | Self::Catalog
            | Self::Workspace
            | Self::WorkspaceList
            | Self::AgentCreate
            | Self::Agent(_)
            | Self::AgentDrive(_)
            | Self::AgentUpdate
            | Self::AgentCommands
            | Self::AgentFilter
            | Self::AgentSubscription
            | Self::ItemsClose
            | Self::TerminalCreate
            | Self::TerminalFilter
            | Self::Terminal
            | Self::TerminalDrive
            | Self::Path
            | Self::Download => true,
        }
    }
}

/// Classify a scoped resource method; whole-node grants use the separate node policy.
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
        "workspace.archive.request" | "workspace.title.set.request" => Rule::WorkspaceManage,
        "workspace.list.request" => Rule::WorkspaceList,
        "workspace.create.request" => Rule::WorkspaceCreate,
        "workspace.worktree.create.request" => Rule::WorktreeCreate,
        "workspace.worktree.archive.request" => Rule::WorktreeArchive,
        "workspace.worktree.list.request" => Rule::Paths(&["cwd", "repoRoot"]),
        "workspace.open.request" => Rule::WorkspaceOpen,
        "project.list.request" => Rule::ProjectList,
        "project.icon.get.request" => Rule::Project,
        "project.config.read.request" => Rule::Paths(&["repoRoot"]),
        "project.add.request"
        | "project.create_directory.request"
        | "project.github.clone.request"
        | "project.rename.request"
        | "project.remove.request"
        | "project.config.write.request"
        | "project.icon.set.request"
        | "workspace.pin.set.request"
        | "workspace.setup.run.request"
        | "workspace.recovery.inspect.request"
        | "workspace.recovery.restore.request"
        | "workspace.github.search_repositories.request" => Rule::Node,
        method if method.starts_with("workspace.label.") => Rule::Node,
        method => classify_resource(method),
    }
}

/// Classify Agent, terminal and file methods.
fn classify_resource(method: &str) -> Rule {
    match method {
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
        "terminal.subscribe.request"
        | "terminal.unsubscribe.request"
        | "terminal.capture.request" => Rule::Terminal,
        "terminal.rename.request" | "terminal.kill.request" => Rule::TerminalDrive,
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

/// Confine one guest request to the guest's shared scope.
pub(crate) async fn request(session: &Session, state: &Shared, mut request: Request) -> Decision {
    if session.kind() == ResourceKind::Node {
        return node::request(session, state, request).await;
    }
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
    if !rule.admits(session.kind()) {
        return Err(ErrorCode::Forbidden);
    }
    match rule {
        Rule::Open | Rule::Node => {}
        Rule::Deny => return Err(ErrorCode::Forbidden),
        Rule::Catalog => optional_path(session, state, &params).await?,
        Rule::Workspace | Rule::WorkspaceManage => {
            workspaces(session, state, &params["workspaceId"]).await?;
        }
        Rule::WorkspaceList | Rule::ProjectList => {
            return Ok(
                match private::prepare(session, rule == Rule::ProjectList, params)? {
                    (params, Some(private)) => Confined::Private(params, private),
                    (params, None) => Confined::Forward(params),
                },
            );
        }
        Rule::WorkspaceCreate
        | Rule::WorktreeCreate
        | Rule::WorktreeArchive
        | Rule::WorkspaceOpen
        | Rule::Project
        | Rule::Paths(_) => directory::confine(session, state, rule, &mut params).await?,
        Rule::TerminalCreate => {
            terminals::place(session, state, &mut params).await?;
            return Ok(Confined::Private(params, Private::TerminalCreate));
        }
        Rule::TerminalFilter => terminals::place(session, state, &mut params).await?,
        Rule::Terminal => {
            let id = params["terminalId"].as_str().ok_or(ErrorCode::Forbidden)?;
            terminals::check(session, state, id).await?;
            if !session.owns_terminal(id) {
                terminals::watch_only(&mut params);
            }
        }
        Rule::TerminalDrive => {
            let id = params["terminalId"].as_str().ok_or(ErrorCode::Forbidden)?;
            terminals::owned(session, id)?;
        }
        Rule::Path => path(session, state, &params).await?,
        Rule::Download => {
            path(session, state, &params).await?;
            if !session.take_download(std::time::Instant::now()) {
                return Err(ErrorCode::ResourceExhausted);
            }
            return Ok(Confined::Private(params, Private::Download));
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
pub(crate) fn event(session: &Session, method: &str, params: &Value) -> EventDecision {
    if session.kind() == ResourceKind::Node
        && node::allows(method, model::methods::InboundKind::Event)
    {
        return EventDecision::Forward;
    }
    match method {
        "session.heartbeat" => EventDecision::Drop,
        "terminal.input" => match params["terminalId"].as_str() {
            Some(id) => match terminals::owned(session, id) {
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

/// Require a workspace ID, or a non-empty list of them, naming only workspaces in scope.
async fn workspaces(session: &Session, state: &Shared, selection: &Value) -> Result<(), ErrorCode> {
    let ids: Vec<&Value> = match selection {
        Value::String(_) => vec![selection],
        Value::Array(ids) if !ids.is_empty() => ids.iter().collect(),
        _ => return Err(ErrorCode::Forbidden),
    };
    for id in ids {
        reached(session, state, id.as_str().ok_or(ErrorCode::Forbidden)?).await?;
    }
    Ok(())
}

/// Require workspace `id` to be inside the scope.
async fn reached(session: &Session, state: &Shared, id: &str) -> Result<(), ErrorCode> {
    if session.reaches(state, id).await.map_err(conceal)? {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Require `path` to resolve inside the scope.
async fn inside(session: &Session, state: &Shared, path: &str) -> Result<(), ErrorCode> {
    if session.contains(state, path).await.map_err(conceal)? {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Require `cwd` inside the scope; node shares reach every path.
async fn path(session: &Session, state: &Shared, params: &Value) -> Result<(), ErrorCode> {
    if session.kind() == ResourceKind::Node {
        return Ok(());
    }
    let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
    inside(session, state, cwd).await
}

async fn optional_path(session: &Session, state: &Shared, params: &Value) -> Result<(), ErrorCode> {
    if present(params.get("cwd")) {
        path(session, state, params).await
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
