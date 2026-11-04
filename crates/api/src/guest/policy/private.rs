//! Guest replies confined through private dispatch, with tracked creation and progress forwarding.

use std::future::Future;
use std::sync::Arc;

use model::outbound::Outbound;
use model::{ErrorCode, Request};
use serde_json::{Value, json};

use super::super::Reach;
use super::{Session, object, present};
use crate::Shared;

/// Reply post-processing for a privately executed guest request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Private {
    /// Keep only workspaces in scope.
    WorkspaceList,
    /// Keep only the shared project.
    ProjectList,
    /// Remember the created terminal so revocation can end it.
    TerminalCreate,
    /// Cancel a newly created Agent if its guest lost authorization during creation.
    AgentCreate,
    /// Bind the issued download token to the visitor, so no other account can redeem it.
    Download,
}

/// Which listed entries a guest keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Keep<'a> {
    /// Only this workspace.
    Workspace(&'a str),
    /// Only this project and its workspaces.
    Project(&'a str),
}

impl Keep<'_> {
    /// The listing filter of `session`; `None` for a node share, which keeps everything.
    fn of(session: &Session) -> Option<Keep<'_>> {
        match &session.reach {
            Reach::Workspace { id, .. } => Some(Keep::Workspace(id)),
            Reach::Project { id, .. } => Some(Keep::Project(id)),
            Reach::Node => None,
        }
    }
}

/// Page size for finding a shared workspace in the directory; tests use single-entry pages.
const WORKSPACE_PAGE: usize = if cfg!(test) { 1 } else { 200 };

/// Pages read at most when looking for a shared workspace.
const MAX_WORKSPACE_PAGES: usize = 100;

/// Prepare a workspace or project listing. A node share forwards it unchanged, live
/// subscription included, because every workspace and project is in scope. Other shares
/// list privately without subscription or synchronization cursor, because those describe
/// the whole directory. A project share narrows the query to its project and keeps its
/// paging, which then covers only that project; a workspace share reads the directory
/// page by page until it finds its workspace.
///
/// # Errors
/// Returns `Forbidden` for a subscription and `InvalidMessage` for non-object parameters.
pub(super) fn prepare(
    session: &Session,
    projects: bool,
    mut params: Value,
) -> Result<(Value, Option<Private>), ErrorCode> {
    let Some(keep) = Keep::of(session) else {
        return Ok((params, None));
    };
    let request = object(&mut params)?;
    if present(request.get("subscribe")) {
        return Err(ErrorCode::Forbidden);
    }
    for key in ["subscribe", "sync"] {
        request.remove(key);
    }
    if projects {
        request.remove("page");
        return Ok((params, Some(Private::ProjectList)));
    }
    match keep {
        Keep::Project(id) => {
            let filter = object(request.entry("filter").or_insert_with(|| json!({})))?;
            filter.insert("projectId".to_owned(), json!(id));
        }
        Keep::Workspace(_) => {
            request.insert("page".to_owned(), json!({"limit": WORKSPACE_PAGE}));
        }
    }
    Ok((params, Some(Private::WorkspaceList)))
}

/// Keep only the entries of `listing` that `keep` admits, discarding everything else. A
/// project listing keeps its page information, which covers only the project; a workspace
/// listing is complete.
pub(super) fn filter_workspaces(listing: &Value, keep: Keep<'_>) -> Value {
    match keep {
        Keep::Workspace(id) => json!({
            "entries": matching(&listing["entries"], "id", id),
            "emptyProjects": [],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
        }),
        Keep::Project(id) => json!({
            "entries": matching(&listing["entries"], "projectId", id),
            "emptyProjects": matching(&listing["emptyProjects"], "projectId", id),
            "pageInfo": {
                "nextCursor": listing["pageInfo"]["nextCursor"],
                "prevCursor": listing["pageInfo"]["prevCursor"],
                "hasMore": listing["pageInfo"]["hasMore"].as_bool().unwrap_or(false),
            },
        }),
    }
}

/// Read the directory page by page until the shared workspace appears.
async fn find_workspace(
    state: &Shared,
    method: &str,
    mut params: Value,
    id: &str,
) -> Result<Value, ErrorCode> {
    for _ in 1..MAX_WORKSPACE_PAGES {
        let reply = crate::connection::rpc::execute(state, method, params.clone()).await?;
        let cursor = &reply["pageInfo"]["nextCursor"];
        if !matching(&reply["entries"], "id", id).is_empty()
            || reply["pageInfo"]["hasMore"] != true
            || !cursor.is_string()
        {
            return Ok(reply);
        }
        params["page"] = json!({"limit": WORKSPACE_PAGE, "cursor": cursor});
    }
    crate::connection::rpc::execute(state, method, params).await
}

/// Keep only project `id` in a project listing.
pub(super) fn filter_projects(listing: &Value, id: &str) -> Value {
    json!({"projects": matching(&listing["projects"], "projectId", id)})
}

fn matching<'a>(items: &'a Value, field: &str, id: &str) -> Vec<&'a Value> {
    items
        .as_array()
        .map(|items| items.iter().filter(|item| item[field] == id).collect())
        .unwrap_or_default()
}

/// Execute an admitted private request and confine its reply.
///
/// # Errors
/// Returns the dispatch error of the underlying request.
pub(crate) async fn execute(
    session: &Session,
    state: &Arc<Shared>,
    method: &str,
    params: Value,
    private: Private,
) -> Result<Value, ErrorCode> {
    if matches!(private, Private::TerminalCreate | Private::AgentCreate) {
        let reply = create(
            session,
            state,
            Creation {
                method: method.to_owned(),
                params,
                private,
                observer: None,
            },
        )
        .await?;
        if let Some(id) = reply["terminal"]["id"].as_str() {
            session.remember_terminal(id);
        }
        return Ok(reply);
    }
    let keep = Keep::of(session);
    let reply = match (private, keep) {
        (Private::WorkspaceList, Some(Keep::Workspace(id))) => {
            find_workspace(state, method, params, id).await?
        }
        _ => crate::connection::rpc::execute(state, method, params).await?,
    };
    match (private, keep) {
        (Private::WorkspaceList, Some(keep)) => Ok(filter_workspaces(&reply, keep)),
        (Private::ProjectList, Some(Keep::Project(id))) => Ok(filter_projects(&reply, id)),
        (
            Private::WorkspaceList
            | Private::ProjectList
            | Private::TerminalCreate
            | Private::AgentCreate,
            _,
        ) => Err(ErrorCode::Forbidden),
        (Private::Download, _) => {
            let Some(token) = reply["token"].as_str().map(str::to_owned) else {
                return Ok(reply);
            };
            let visitor = session.registration.grant().user_id.to_string();
            let bound = state
                .run(
                    state.filesystem.files.clone(),
                    ErrorCode::ProjectIo,
                    move |files| Ok(files.bind_download(&token, &visitor)),
                )
                .await?;
            // The token is only useful once bound; an unbound one would serve the owner.
            if bound {
                Ok(reply)
            } else {
                Err(ErrorCode::Forbidden)
            }
        }
    }
}

struct Creation {
    method: String,
    params: Value,
    private: Private,
    observer: Option<(Outbound, usize)>,
}

/// Preserve creation progress on the guest connection while detached work owns cleanup.
/// # Errors
/// Returns creation errors or `Forbidden` when the grant is revoked during creation.
pub(crate) async fn create_observed(
    session: &Session,
    state: &Arc<Shared>,
    request: Request,
    observer: (Outbound, usize),
) -> Result<Value, ErrorCode> {
    create(
        session,
        state,
        Creation {
            method: request.method,
            params: request.params,
            private: Private::AgentCreate,
            observer: Some(observer),
        },
    )
    .await
}

/// Finish admitted creation and its revocation check even if the connection is dropped.
async fn create(
    session: &Session,
    state: &Arc<Shared>,
    creation: Creation,
) -> Result<Value, ErrorCode> {
    let state = state.clone();
    let grant = session.registration.grant();
    let cancellation = session.cancellation().clone();
    state
        .runtime
        .tasks
        .clone()
        .spawn(async move {
            if cancellation.is_cancelled() || !state.shares.grants(grant) {
                return Err(ErrorCode::Forbidden);
            }
            let reply = crate::connection::rpc::execute_observed(
                &state,
                &creation.method,
                creation.params,
                creation.observer,
            )
            .await?;
            match creation.private {
                Private::TerminalCreate => {
                    if let Some(id) = reply["terminal"]["id"].as_str() {
                        if cancellation.is_cancelled()
                            || !state.shares.record_terminal(grant, id.to_owned())
                        {
                            terminal::dispatch::revoke(&state.terminal, vec![id.to_owned()])
                                .await?;
                            return Err(ErrorCode::Forbidden);
                        }
                        prune_terminals(&state).await;
                    }
                }
                Private::AgentCreate => {
                    finish_agent(
                        &reply,
                        !cancellation.is_cancelled() && state.shares.grants(grant),
                        |id| async move {
                            crate::connection::rpc::execute(
                                &state,
                                "agent.cancel.request",
                                json!({"agentId": id}),
                            )
                            .await
                            .map(|_| ())
                        },
                    )
                    .await?;
                }
                Private::WorkspaceList | Private::ProjectList | Private::Download => {
                    return Err(ErrorCode::Forbidden);
                }
            }
            Ok(reply)
        })
        .await
        .map_err(|_| ErrorCode::ServerDraining)?
}

/// Remove only previously recorded IDs that a later terminal listing confirms are gone.
/// IDs added during the listing remain tracked, avoiding races with concurrent creation.
async fn prune_terminals(state: &Shared) {
    let recorded = state.shares.terminal_ids();
    let Ok(listed) =
        crate::connection::rpc::execute(state, "terminal.list.request", json!({})).await
    else {
        return;
    };
    let Some(terminals) = listed["terminals"].as_array() else {
        return;
    };
    let exited = recorded
        .into_iter()
        .filter(|id| {
            !terminals
                .iter()
                .any(|terminal| terminal["id"] == id.as_str())
        })
        .collect::<Vec<_>>();
    state.shares.forget_terminals(&exited);
}

/// Cancel a creation that completed after revocation, including an embedded Workspace Agent.
async fn finish_agent<F, R>(reply: &Value, authorized: bool, cancel: F) -> Result<(), ErrorCode>
where
    F: FnOnce(String) -> R,
    R: Future<Output = Result<(), ErrorCode>>,
{
    if authorized {
        return Ok(());
    }
    if let Some(id) = reply["agentId"]
        .as_str()
        .or_else(|| reply["agent"]["id"].as_str())
    {
        cancel(id.to_owned()).await?;
    }
    Err(ErrorCode::Forbidden)
}

#[cfg(test)]
mod tests;
