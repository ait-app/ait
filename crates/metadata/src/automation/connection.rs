//! Workspace-scoped setup transcripts with a paused, ordered bootstrap.

use std::sync::Arc;

use domain::session::SessionError;
use model::outbound::QueueError;
use model::{Context, ErrorCode, ServerMessage};
use serde_json::json;

use crate::automation::protocol::WorkspaceSetupRequest;
use crate::connection::Connection;
use crate::dispatch::State;

pub(crate) async fn subscribe(
    mut context: Context<'_>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    let prepared = (|| {
        if context.available_subscriptions == 0 {
            return Err(ErrorCode::ResourceExhausted);
        }
        if state.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        if state.workspace_automation.is_none() {
            return Err(ErrorCode::UnsupportedCapability);
        }
        let request: WorkspaceSetupRequest = serde_json::from_value(context.request.params.clone())
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let sink = context.outbound.clone();
        connection
            .session
            .as_ref()
            .ok_or(ErrorCode::InvalidMessage)?
            .subscribe_workspace_setup(
                request.workspace_id,
                Arc::new(move |kind, params| {
                    sink.send(&ServerMessage::Event {
                        method: kind.method().to_owned(),
                        params,
                    })
                    .map_err(|_| SessionError::Closed)
                }),
            )
            .map_err(|_| ErrorCode::InvalidMessage)
    })();
    let subscription = match prepared {
        Ok(subscription) => subscription,
        Err(error) => return context.respond(Err(error)),
    };
    let result = context
        .call(
            state.workspace_automation.clone(),
            ErrorCode::RegistryIo,
            |service, _, params| {
                crate::automation::rpc::execute(service, "workspace.setup.status.request", params)
            },
        )
        .await;
    let mut snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => return context.respond(Err(error)),
    };
    let id = subscription.id().to_owned();
    snapshot["subscriptionId"] = json!(id);
    let outbound = context.outbound;
    context.respond(Ok(snapshot))?;
    if subscription.activate().is_err() {
        outbound.failure().cancel();
        return Err(QueueError::Full);
    }
    connection.events.insert(id, subscription);
    Ok(())
}
