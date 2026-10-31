//! Guest requests executed once without subscriptions so their replies can be confined.

use model::ErrorCode;
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
    state: &Shared,
    method: &str,
    params: Value,
    private: Private,
) -> Result<Value, ErrorCode> {
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
        (Private::WorkspaceList | Private::ProjectList, _) => Err(ErrorCode::Forbidden),
        (Private::TerminalCreate, _) => {
            let Some(id) = reply["terminal"]["id"].as_str() else {
                return Ok(reply);
            };
            if !session.registration.record_terminal(id.to_owned()) {
                // The share was revoked while the terminal started.
                terminal::dispatch::revoke(&state.terminal, vec![id.to_owned()]).await?;
                return Err(ErrorCode::Forbidden);
            }
            session.remember_terminal(id);
            Ok(reply)
        }
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
