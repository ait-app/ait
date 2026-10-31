//! Terminal methods. A visitor watches terminals of workspaces inside the shared scope,
//! but types into, resizes, renames and kills only terminals it created: the owner's or
//! another visitor's shell may run outside the scope or hold cached credentials.

use serde_json::{Value, json};

use super::super::Reach;
use super::{ErrorCode, Session, conceal, inside, object, present, reached};
use crate::Shared;

/// Place a terminal creation or listing in a workspace in scope. A workspace share forces
/// its workspace and directory; a project share requires a workspace of the project and
/// defaults the directory to it; a node share keeps the request. A given `cwd` must be in
/// scope.
pub(super) async fn place(
    session: &Session,
    state: &Shared,
    params: &mut Value,
) -> Result<(), ErrorCode> {
    if matches!(session.reach, Reach::Node) {
        return Ok(());
    }
    let directory = placement(session, state, params).await?;
    if let Some(cwd) = params.get("cwd").filter(|cwd| !cwd.is_null()) {
        inside(session, state, cwd.as_str().ok_or(ErrorCode::Forbidden)?).await?;
    }
    let object = object(params)?;
    if !present(object.get("cwd")) {
        object.insert("cwd".to_owned(), json!(directory));
    }
    Ok(())
}

/// Resolve the workspace a terminal request targets and pin it in `params`, returning
/// the workspace directory. A workspace share always targets its workspace; a project
/// share must name a workspace of the project.
async fn placement(
    session: &Session,
    state: &Shared,
    params: &mut Value,
) -> Result<String, ErrorCode> {
    let id = match &session.reach {
        Reach::Workspace { id, .. } => id.clone(),
        Reach::Project { .. } | Reach::Node => params["workspaceId"]
            .as_str()
            .ok_or(ErrorCode::Forbidden)?
            .to_owned(),
    };
    let directory = session
        .directory(state, &id)
        .await
        .map_err(conceal)?
        .ok_or(ErrorCode::Forbidden)?;
    object(params)?.insert("workspaceId".to_owned(), json!(id));
    Ok(directory)
}

/// Require terminal `id` to have been created by this visitor through this share.
///
/// # Errors
/// Returns `Forbidden` for every other terminal.
pub(crate) fn owned(session: &Session, id: &str) -> Result<(), ErrorCode> {
    if session.owns_terminal(id) {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Drop the size claim of a subscription, which would take over the terminal's size.
pub(super) fn watch_only(params: &mut Value) {
    if let Some(restore) = params.get_mut("restore").and_then(Value::as_object_mut) {
        restore.remove("size");
    }
}

/// Require terminal `id` to belong to a workspace in scope.
pub(super) async fn check(session: &Session, state: &Shared, id: &str) -> Result<(), ErrorCode> {
    if matches!(session.reach, Reach::Node) || session.knows_terminal(id) {
        return Ok(());
    }
    let filter = match &session.reach {
        Reach::Workspace { id, .. } => json!({"workspaceId": id}),
        Reach::Project { .. } | Reach::Node => json!({}),
    };
    let listed = crate::connection::rpc::execute(state, "terminal.list.request", filter)
        .await
        .map_err(conceal)?;
    let workspace = listed["terminals"]
        .as_array()
        .and_then(|terminals| terminals.iter().find(|terminal| terminal["id"] == id))
        .and_then(|terminal| terminal["workspaceId"].as_str())
        .ok_or(ErrorCode::Forbidden)?;
    reached(session, state, workspace).await?;
    session.remember_terminal(id);
    Ok(())
}
