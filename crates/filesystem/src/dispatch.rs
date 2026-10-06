//! Concrete service state and crate-owned request dispatch.

mod metadata;

use std::sync::{Arc, Mutex};

use model::outbound::QueueError;
use model::{Context, ErrorCode, Runtime};

use crate::capabilities::{Group, IMPLEMENTED_GROUPS};

/// Services installed for this capability crate, sharing server-wide runtime resources.
#[derive(Debug)]
pub struct State {
    /// Optional model-backed wording service; unavailable generation uses deterministic fallbacks.
    pub metadata_generator: Option<Arc<dyn ::metadata::ports::generation::MetadataGenerator>>,
    /// Installed skills service.
    pub skills: Option<Arc<Mutex<crate::service::skills::Skills>>>,
    /// Shared Tokio admission, cancellation and task tracking.
    pub runtime: Arc<Runtime>,
    /// Installed checkout service.
    pub checkout: Option<Arc<Mutex<crate::service::checkout::Checkout>>>,
    /// Installed forge service.
    pub forge: Option<Arc<Mutex<crate::service::forge::Forge>>>,
    /// Installed files service.
    pub files: Option<Arc<Mutex<crate::service::files::Files>>>,
    /// Installed github projects service.
    pub github_projects: Option<Arc<Mutex<crate::service::github_projects::GithubProjects>>>,
    /// Installed worktrees service.
    pub worktrees: Option<Arc<Mutex<crate::service::worktrees::Worktrees>>>,
    /// Installed workspace recovery service.
    pub workspace_recovery:
        Option<Arc<Mutex<crate::service::workspace_recovery::WorkspaceRecovery>>>,
    /// Installed workspace automation service.
    pub workspace_automation:
        Option<Arc<Mutex<::metadata::service::workspace_automation::WorkspaceAutomation>>>,
}

impl std::ops::Deref for State {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

use crate::connection::Connection;
use model::valid_id;
use serde_json::{Value, json};

/// Dispatch an admitted request through filesystem-owned services and connection state.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns delivery failures; business failures use the request's error envelope.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    let Some((group, mut context)) = Context::take_matching(context, IMPLEMENTED_GROUPS) else {
        return Ok(());
    };
    if matches!(group, Group::Checkout | Group::Forge)
        && let Err(error) =
            metadata::fill(state, &context.request.method, &mut context.request.params).await
    {
        return context.respond(Err(error));
    }
    match group {
        Group::Skills => {
            context
                .rpc(
                    state.skills.clone(),
                    ErrorCode::RegistryIo,
                    crate::service::skills::Skills::execute,
                )
                .await
        }
        Group::Checkout => checkout(context, state, connection).await,
        Group::Forge => {
            context
                .rpc(
                    state.forge.clone(),
                    ErrorCode::ProjectIo,
                    |service, method, params| crate::rpc::forge::execute(service, method, params),
                )
                .await
        }
        Group::Files => {
            connection
                .files
                .request(
                    crate::connection::files::FileRequest {
                        id: context.request.id,
                        method: context.request.method,
                        params: context.request.params,
                        available_subscriptions: context.available_subscriptions,
                    },
                    state,
                    context.outbound,
                )
                .await
        }
        Group::GithubProjects => {
            context
                .rpc(
                    state.github_projects.clone(),
                    ErrorCode::RegistryIo,
                    |service, method, params| {
                        crate::rpc::github_projects::execute(service, method, params)
                    },
                )
                .await
        }
        Group::Worktrees => worktrees(context, state).await,
        Group::WorkspaceRecovery => {
            let result = context
                .call(
                    state.workspace_recovery.clone(),
                    ErrorCode::RegistryIo,
                    |service, method, params| {
                        crate::rpc::workspace_recovery::execute(service, method, params)
                    },
                )
                .await;
            match result {
                Ok(reply) => context.workspace(reply.value, reply.event),
                Err(error) => context.respond(Err(error)),
            }
        }
    }
}

async fn worktrees(mut context: Context<'_>, state: &State) -> Result<(), QueueError> {
    let result = context
        .call(
            state.worktrees.clone(),
            ErrorCode::RegistryIo,
            crate::rpc::worktrees::execute,
        )
        .await;
    if let Ok(reply) = &result
        && let Some(workspace_id) = reply.created_workspace_id.clone()
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
    }
    match result {
        Ok(reply) => context.workspace(reply.value, reply.event),
        Err(error) => context.respond(Err(error)),
    }
}

async fn checkout(
    mut context: Context<'_>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), QueueError> {
    if context.request.method == "checkout.diff.unsubscribe.request" {
        let request = serde_json::from_value::<
            crate::protocol::checkout::CheckoutDiffUnsubscribeRequest,
        >(std::mem::take(&mut context.request.params));
        let request = match request {
            Ok(request) if valid_id(&request.subscription_id) => request,
            _ => return context.respond(Err(ErrorCode::InvalidMessage)),
        };
        if connection.diffs.remove(&request.subscription_id).is_none() {
            return context.respond(Err(ErrorCode::SubscriptionNotFound));
        }
        return context.respond(Ok(json!({"subscriptionId":request.subscription_id})));
    }
    if context.request.method == "checkout.diff.subscribe.request" {
        let replacement = context
            .request
            .params
            .get("subscriptionId")
            .and_then(Value::as_str)
            .is_some_and(|id| connection.diffs.contains_key(id));
        if context.available_subscriptions == 0 && !replacement {
            return context.respond(Err(ErrorCode::ResourceExhausted));
        }
    }
    let params = std::mem::take(&mut context.request.params);
    let reply = crate::connection::checkout::dispatch(
        &context.request.method,
        params,
        state,
        context.outbound.clone(),
    )
    .await;
    match reply {
        Ok(reply) => {
            context.respond(Ok(reply.value))?;
            if let Some(pending) = reply.subscription {
                let (id, subscription) = pending.activate();
                connection.diffs.insert(id, subscription);
            }
            Ok(())
        }
        Err(error) => context.respond(Err(error)),
    }
}
