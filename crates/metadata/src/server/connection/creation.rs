//! Durable creation receipt snapshots and observers owned by one physical connection.

use domain::creation::protocol::SubscribeRequest;
use model::events::Subscription;
use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::{Value, json};

use crate::connection::Connection;
use crate::dispatch::State;

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
