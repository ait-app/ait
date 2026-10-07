//! Completed receipts reference existing resources; observation must not recreate missing ones.

use std::sync::{Arc, Mutex};

use model::creation::protocol::{Kind, Snapshot, SubscribeRequest};
use model::{ErrorCode, Request};

use crate::Shared;

pub(super) async fn validate(request: &Request, state: &Shared) -> Result<(), ErrorCode> {
    let request: SubscribeRequest =
        serde_json::from_value(request.params.clone()).map_err(|_| ErrorCode::InvalidMessage)?;
    let creations = state.metadata.creations.clone();
    let snapshot = state
        .runtime
        .run(
            Some(Arc::new(Mutex::new(creations))),
            ErrorCode::RegistryIo,
            move |creations| creations.snapshot(request.kind, &request.idempotency_key),
        )
        .await?;
    let Some(snapshot) = snapshot.filter(|snapshot| snapshot.phase == "completed") else {
        return Ok(());
    };
    if snapshot.kind == Kind::Workspace
        && snapshot.workspace.is_some()
        && let Some(id) = snapshot.workspace_id.clone()
    {
        let exists = state
            .runtime
            .run(
                state.metadata.directory.clone(),
                ErrorCode::RegistryIo,
                move |directory| {
                    directory
                        .contains_workspace_directory(&id)
                        .map_err(|_| ErrorCode::RegistryIo)
                },
            )
            .await?;
        if !exists {
            return Err(ErrorCode::WorkspaceNotFound);
        }
    }
    validate_agent(state, &snapshot).await
}

pub(super) async fn validate_agent(state: &Shared, snapshot: &Snapshot) -> Result<(), ErrorCode> {
    if snapshot.phase == "completed"
        && let Some(id) = snapshot.agent_id.clone()
        && !provider::dispatch::contains_identity(&state.provider, id).await?
    {
        return Err(ErrorCode::AgentNotFound);
    }
    Ok(())
}
