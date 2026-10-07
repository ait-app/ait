use model::events::Subscription;
use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::connection::Connection;
use crate::dispatch::State;
use crate::protocol::creation::{Kind, SubscribeRequest};

pub(crate) async fn dispatch(
    context: Context<'_>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    let request: SubscribeRequest = match serde_json::from_value(context.request.params.clone()) {
        Ok(request) => request,
        Err(_) => return context.respond(Err(ErrorCode::InvalidMessage)),
    };
    let observe = request.subscribe.unwrap_or(true);
    if observe && context.available_subscriptions == 0 {
        return context.respond(Err(ErrorCode::ResourceExhausted));
    }
    let creations = state.creations.clone();
    // File-backed receipt reads stay outside the WebSocket reactor.
    let outbound = context.outbound.clone();
    let result = context
        .runtime
        .run(
            Some(std::sync::Arc::new(std::sync::Mutex::new(creations))),
            ErrorCode::RegistryIo,
            move |creations| {
                if observe {
                    creations
                        .subscribe(request.kind, &request.idempotency_key, outbound)
                        .map(|(snapshot, subscription)| {
                            (
                                json!({"snapshot":snapshot,"error":null}),
                                Some(subscription),
                            )
                        })
                } else {
                    creations
                        .snapshot(request.kind, &request.idempotency_key)
                        .map(|snapshot| (json!({"snapshot":snapshot,"error":null}), None))
                }
            },
        )
        .await;
    deliver(context, result, connection, None)
}

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
            |directory, _, params| crate::rpc::directory::workspace_creation(directory, params),
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

fn deliver(
    context: Context<'_>,
    result: Result<(Value, Option<Subscription>), ErrorCode>,
    connection: &mut Connection,
    event: Option<Value>,
) -> Result<(), QueueError> {
    match result {
        Ok((mut value, subscription)) => {
            if let Some(subscription) = &subscription {
                value["subscriptionId"] = json!(subscription.id());
            }
            context.workspace(value, event)?;
            if let Some(subscription) = subscription {
                subscription.activate()?;
                connection
                    .creations
                    .insert(subscription.id().to_owned(), subscription);
            }
            Ok(())
        }
        Err(error) => context.respond(Err(error)),
    }
}
