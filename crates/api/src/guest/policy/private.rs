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

/// Prepare a workspace or project listing. A node share forwards it unchanged, live
/// subscription included, because every workspace and project is in scope. Other shares
/// list privately without subscription, paging or synchronization cursor, because those
/// describe the whole directory; a project share also narrows the query to its project.
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
    for key in ["subscribe", "page", "sync"] {
        request.remove(key);
    }
    if projects {
        return Ok((params, Some(Private::ProjectList)));
    }
    if let Keep::Project(id) = keep {
        let filter = object(request.entry("filter").or_insert_with(|| json!({})))?;
        filter.insert("projectId".to_owned(), json!(id));
    }
    Ok((params, Some(Private::WorkspaceList)))
}

/// Keep only the entries of `listing` that `keep` admits, discarding everything else.
pub(super) fn filter_workspaces(listing: &Value, keep: Keep<'_>) -> Value {
    let (field, id) = match keep {
        Keep::Workspace(id) => ("id", id),
        Keep::Project(id) => ("projectId", id),
    };
    let empty_projects = match keep {
        Keep::Workspace(_) => Vec::new(),
        Keep::Project(id) => matching(&listing["emptyProjects"], "projectId", id),
    };
    json!({
        "entries": matching(&listing["entries"], field, id),
        "emptyProjects": empty_projects,
        "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
    })
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
    let reply = crate::connection::rpc::execute(state, method, params).await?;
    match (private, Keep::of(session)) {
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
    }
}
