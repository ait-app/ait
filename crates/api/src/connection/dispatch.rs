use model::outbound::{Outbound, QueueError};
use model::{Context, DispatchError, ErrorCode};

use super::ConnectionSubscriptions;
use crate::Shared;

pub(super) async fn request(
    context: Context<'_>,
    state: &Shared,
    subscriptions: &mut ConnectionSubscriptions,
) -> Result<(), QueueError> {
    if context.request.method == "creation.subscribe.request"
        && let Err(error) = super::creation_receipts::validate(&context.request, state).await
    {
        return context.respond(Err(error));
    }
    let outbound = context.outbound;
    let mut context = Some(context);
    match super::workspace_creation::request(&mut context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(&context, "api::workspace_creation");
        }
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match super::workspace_archive::request(&mut context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(&context, "api::workspace_archive");
        }
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match crate::relay_rpc::request(&mut context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "api::relay"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match schedule::dispatch::dispatch(&mut context, &state.schedule).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "schedule"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match browser::dispatch::dispatch(&mut context, &state.browser, &mut subscriptions.browser) {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "browser"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match voice::dispatch::dispatch(&mut context, &state.voice, &mut subscriptions.voice).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "voice"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match metadata::dispatch::dispatch(&mut context, &state.metadata, &mut subscriptions.metadata)
        .await
    {
        Ok(Some(completion)) => {
            return complete_metadata(completion, state, subscriptions, outbound).await;
        }
        Ok(None) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "metadata"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match filesystem::dispatch::dispatch(
        &mut context,
        &state.filesystem,
        &mut subscriptions.filesystem,
    )
    .await
    {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "filesystem"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match provider::dispatch::dispatch(&mut context, &state.provider, &mut subscriptions.provider)
        .await
    {
        Ok(Some(provider::dispatch::Completion::CloseTerminals {
            request_id,
            mut value,
            terminal_ids,
        })) => {
            let result = terminal::dispatch::close_many(&state.terminal, terminal_ids)
                .await
                .map(|terminals| {
                    value["terminals"] = terminals;
                    value
                });
            return outbound.respond(request_id, result);
        }
        Ok(None) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "provider"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match terminal::dispatch::dispatch(&mut context, &state.terminal, &mut subscriptions.terminals)
        .await
    {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(&context, "terminal"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    context
        .expect("All handlers declined without consuming the request")
        .respond(Err(ErrorCode::NotImplemented))
}

async fn complete_metadata(
    completion: metadata::dispatch::Completion,
    state: &Shared,
    subscriptions: &mut ConnectionSubscriptions,
    outbound: &Outbound,
) -> Result<(), QueueError> {
    match completion {
        metadata::dispatch::Completion::DaemonSnapshot {
            request_id,
            method,
            params,
        } => {
            let result = daemon_snapshot(state, method, params).await;
            outbound.respond(request_id, result)
        }
        metadata::dispatch::Completion::Release {
            request_id,
            subscription_id,
        } => {
            subscriptions.release(&subscription_id);
            let value = serde_json::to_value(model::subscription::SubscriptionReleaseResult {
                subscription_id,
            })
            .map_err(|_| ErrorCode::InvalidMessage);
            outbound.respond(request_id, value)
        }
    }
}

async fn daemon_snapshot(
    state: &Shared,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, ErrorCode> {
    let providers = match &state.provider.agent_execution {
        Some(execution) => {
            let value = execution
                .execute("provider.available.list.request", serde_json::json!({}))
                .await?;
            serde_json::from_value(value["providers"].clone()).map_err(|_| ErrorCode::AgentIo)?
        }
        None => Vec::new(),
    };
    metadata::dispatch::daemon_snapshot(&state.metadata, method, params, providers).await
}

#[cfg(test)]
mod tests;
