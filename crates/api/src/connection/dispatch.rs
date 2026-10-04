use model::outbound::QueueError;
use model::{Context, ErrorCode};

use super::ConnectionSubscriptions;
use crate::Shared;
use crate::capabilities::Group;

pub(super) async fn request(
    group: Group,
    context: Context<'_>,
    state: &Shared,
    subscriptions: &mut ConnectionSubscriptions,
) -> Result<(), QueueError> {
    if context.request.method == "creation.subscribe.request"
        && let Err(error) = super::creation_receipts::validate(&context.request, state).await
    {
        return context.respond(Err(error));
    }
    if super::workspace_creation::handles(&context.request) {
        return super::workspace_creation::request(context, state).await;
    }
    if super::workspace_archive::handles(&context.request.method) {
        return super::workspace_archive::request(context, state).await;
    }
    dispatch_group(group, context, state, subscriptions).await
}

async fn dispatch_group(
    group: Group,
    context: Context<'_>,
    state: &Shared,
    subscriptions: &mut ConnectionSubscriptions,
) -> Result<(), QueueError> {
    let outbound = context.outbound;
    match group {
        Group::Relay => crate::relay_rpc::request(context, state).await,
        Group::Schedule(group) => {
            schedule::dispatch::dispatch(group, context, &state.schedule).await
        }
        Group::Browser(group) => {
            browser::dispatch::dispatch(group, context, &state.browser, &mut subscriptions.browser)
        }
        Group::Voice(group) => {
            voice::dispatch::dispatch(group, context, &state.voice, &mut subscriptions.voice).await
        }
        Group::Metadata(group) => {
            match metadata::dispatch::dispatch(
                group,
                context,
                &state.metadata,
                &mut subscriptions.metadata,
            )
            .await?
            {
                metadata::dispatch::Completion::Complete => Ok(()),
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
                    let value =
                        serde_json::to_value(model::subscription::SubscriptionReleaseResult {
                            subscription_id,
                        })
                        .map_err(|_| ErrorCode::InvalidMessage);
                    outbound.respond(request_id, value)
                }
            }
        }
        Group::Filesystem(group) => {
            filesystem::dispatch::dispatch(
                group,
                context,
                &state.filesystem,
                &mut subscriptions.filesystem,
            )
            .await
        }
        Group::Provider(group) => {
            match provider::dispatch::dispatch(
                group,
                context,
                &state.provider,
                &mut subscriptions.provider,
            )
            .await?
            {
                provider::dispatch::Completion::Complete => Ok(()),
                provider::dispatch::Completion::CloseTerminals {
                    request_id,
                    mut value,
                    terminal_ids,
                } => {
                    let result = terminal::dispatch::close_many(&state.terminal, terminal_ids)
                        .await
                        .map(|terminals| {
                            value["terminals"] = terminals;
                            value
                        });
                    outbound.respond(request_id, result)
                }
            }
        }
        Group::Terminal(group) => {
            terminal::dispatch::dispatch(
                group,
                context,
                &state.terminal,
                &mut subscriptions.terminals,
            )
            .await
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
