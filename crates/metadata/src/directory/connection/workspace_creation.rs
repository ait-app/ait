//! Workspace creation with optional receipt observation and post-creation setup.

use domain::creation::protocol::Kind;
use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::dispatch::State;

pub(crate) async fn create(mut context: Context<'_>, state: &State) -> Result<(), QueueError> {
    if !context.request.params.is_object() {
        return context.respond(Err(ErrorCode::InvalidMessage));
    }
    let observe = context
        .request
        .params
        .get("subscribe")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if observe && context.available_subscriptions == 0 {
        return context.respond(Err(ErrorCode::ResourceExhausted));
    }
    let key = match context.request.params.get("idempotencyKey") {
        Some(Value::String(key)) => key.clone(),
        None | Some(Value::Null) => Uuid::new_v4().to_string(),
        _ => return context.respond(Err(ErrorCode::InvalidMessage)),
    };
    context.request.params["idempotencyKey"] = json!(key);
    let subscription = if observe {
        let creations = state.creations.clone();
        let outbound = context.outbound.clone();
        let result = context
            .runtime
            .run(
                Some(std::sync::Arc::new(std::sync::Mutex::new(creations))),
                ErrorCode::RegistryIo,
                move |creations| creations.observe(Kind::Workspace, &key, outbound),
            )
            .await;
        match result {
            Ok(subscription) => Some(subscription),
            Err(error) => return context.respond(Err(error)),
        }
    } else {
        None
    };
    if let Some(subscription) = &subscription {
        subscription.activate()?;
    }
    let result = context
        .call(
            state.directory.clone(),
            ErrorCode::RegistryIo,
            |directory, _, params| {
                crate::directory::rpc::directory::workspace_creation(directory, params)
            },
        )
        .await;
    let event = if let Ok(reply) = &result
        && let Some(workspace_id) = reply.created_worktree_id.clone()
    {
        let _ = state
            .run(
                state.workspace_automation.clone(),
                ErrorCode::RegistryIo,
                move |automation| {
                    automation
                        .start_created_setup(&workspace_id)
                        .map(|_| ())
                        .map_err(|_| ErrorCode::RegistryIo)
                },
            )
            .await;
        Some(json!({"kind":"upsert", "workspace":reply.value["workspace"]}))
    } else {
        None
    };
    match result {
        Ok(reply) => context.workspace(reply.value, event),
        Err(error) => context.respond(Err(error)),
    }
}
