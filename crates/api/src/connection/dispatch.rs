use model::outbound::{Outbound, QueueError};
use model::{Context, ErrorCode};

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
    super::workspace_creation::request(&mut context, state).await?;
    if context.is_none() {
        return Ok(());
    }
    super::workspace_archive::request(&mut context, state).await?;
    if context.is_none() {
        return Ok(());
    }
    crate::relay_rpc::request(&mut context, state).await?;
    if context.is_none() {
        return Ok(());
    }
    schedule::dispatch::dispatch(&mut context, &state.schedule).await?;
    if context.is_none() {
        return Ok(());
    }
    browser::dispatch::dispatch(&mut context, &state.browser, &mut subscriptions.browser)?;
    if context.is_none() {
        return Ok(());
    }
    voice::dispatch::dispatch(&mut context, &state.voice, &mut subscriptions.voice).await?;
    if context.is_none() {
        return Ok(());
    }
    if let Some(completion) =
        metadata::dispatch::dispatch(&mut context, &state.metadata, &mut subscriptions.metadata)
            .await?
    {
        complete_metadata(completion, state, subscriptions, outbound).await?;
    }
    if context.is_none() {
        return Ok(());
    }
    filesystem::dispatch::dispatch(
        &mut context,
        &state.filesystem,
        &mut subscriptions.filesystem,
    )
    .await?;
    if context.is_none() {
        return Ok(());
    }
    if let Some(provider::dispatch::Completion::CloseTerminals {
        request_id,
        mut value,
        terminal_ids,
    }) = provider::dispatch::dispatch(&mut context, &state.provider, &mut subscriptions.provider)
        .await?
    {
        let result = terminal::dispatch::close_many(&state.terminal, terminal_ids)
            .await
            .map(|terminals| {
                value["terminals"] = terminals;
                value
            });
        outbound.respond(request_id, result)?;
    }
    if context.is_none() {
        return Ok(());
    }
    terminal::dispatch::dispatch(&mut context, &state.terminal, &mut subscriptions.terminals)
        .await?;
    if let Some(context) = context {
        return context.respond(Err(ErrorCode::NotImplemented));
    }
    Ok(())
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
