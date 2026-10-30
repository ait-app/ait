//! Owner RPC methods for share settings; guest connections never reach them.

use domain::workspace::records::PersistedWorkspaceRecord;
use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode, Request};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::setting::{RemoveRequest, SetRequest, ShareError};
use crate::Shared;

/// Share methods owned by the API crate.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("share.list.request"),
    MethodSpec::request("share.set.request"),
    MethodSpec::request("share.remove.request"),
    MethodSpec::request("share.orgs.list.request"),
];

/// Handle a share method, leaving other requests for later handlers.
///
/// # Errors
/// Returns `NotImplemented` for other methods and `Delivery` when the reply cannot be queued.
pub(crate) async fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), DispatchError> {
    let Some(context) = context.take_if(|context| {
        METHODS
            .iter()
            .any(|method| method.name == context.request.method)
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = execute(&context.request, state).await;
    context.respond(result).map_err(Into::into)
}

async fn execute(request: &Request, state: &Shared) -> Result<Value, ErrorCode> {
    match request.method.as_str() {
        "share.list.request" => Ok(state.shares.list()),
        "share.orgs.list.request" => Ok(state.shares.orgs()),
        "share.set.request" => {
            let set: SetRequest = decode(&request.params)?;
            active_workspace(state, &set.workspace_key)
                .await?
                .ok_or(ShareError::Workspace)?;
            state.shares.set(set).await
        }
        "share.remove.request" => {
            let remove: RemoveRequest = decode(&request.params)?;
            state
                .shares
                .remove(&remove.workspace_key, remove.organization_id)
                .await
        }
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn decode<T: DeserializeOwned>(params: &Value) -> Result<T, ErrorCode> {
    T::deserialize(params).map_err(|_| ErrorCode::InvalidMessage)
}

/// Read workspace `id` when it exists and is not archived.
///
/// # Errors
/// Returns `UnsupportedCapability` without a directory, or registry failures.
pub(crate) async fn active_workspace(
    state: &Shared,
    id: &str,
) -> Result<Option<PersistedWorkspaceRecord>, ErrorCode> {
    let id = id.to_owned();
    state
        .runtime
        .run_queued(
            state.metadata.directory.clone(),
            ErrorCode::RegistryIo,
            move |directory| {
                let workspaces = directory
                    .list_workspaces()
                    .map_err(|_| ErrorCode::RegistryIo)?;
                Ok(workspaces.into_iter().find(|workspace| {
                    workspace.workspace_id == id
                        && workspace.archived_at.as_ref().is_none_or(String::is_empty)
                }))
            },
        )
        .await
}

#[cfg(test)]
mod tests;
