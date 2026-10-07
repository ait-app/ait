//! Concrete service state and crate-owned request dispatch.

/// Client methods implemented by this component; consumed by capability discovery.
pub const CHECKOUT_METHODS: &[MethodSpec] = &[
    MethodSpec::request("checkout.status.get.request"),
    MethodSpec::request("checkout.refresh.request"),
    MethodSpec::request("checkout.diff.get.request"),
    MethodSpec::request("checkout.diff.subscribe.request"),
    MethodSpec::request("checkout.diff.unsubscribe.request"),
    MethodSpec::request("checkout.commits.list.request"),
    MethodSpec::request("checkout.commits.file_diff.request"),
    MethodSpec::request("checkout.branch.validate.request"),
    MethodSpec::request("checkout.branch.suggestions.request"),
    MethodSpec::request("checkout.branch.switch.request"),
    MethodSpec::request("checkout.rename_branch.request"),
    MethodSpec::request("checkout.commit.request"),
    MethodSpec::request("checkout.merge.request"),
    MethodSpec::request("checkout.merge_from_base.request"),
    MethodSpec::request("checkout.reset_workspace.request"),
    MethodSpec::request("checkout.pull.request"),
    MethodSpec::request("checkout.push.request"),
    MethodSpec::request("checkout.discard_changes.request"),
    MethodSpec::request("checkout.stash.save.request"),
    MethodSpec::request("checkout.stash.pop.request"),
    MethodSpec::request("checkout.stash.list.request"),
];

mod metadata;

use std::sync::{Arc, Mutex};

use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode, Runtime};

mod requests;

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
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business failures use the request's error envelope.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    match requests::skills(context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::skills");
        }
        Err(error) => return Err(error),
    }
    match checkout(context, state, connection).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::checkout");
        }
        Err(error) => return Err(error),
    }
    match requests::forge(context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::forge");
        }
        Err(error) => return Err(error),
    }
    match requests::files(context, state, connection).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::files");
        }
        Err(error) => return Err(error),
    }
    match requests::github_projects(context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::github_projects");
        }
        Err(error) => return Err(error),
    }
    match worktrees(context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::worktrees");
        }
        Err(error) => return Err(error),
    }
    match requests::recovery(context, state).await {
        Ok(()) => return Ok(()),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "filesystem::recovery");
        }
        Err(error) => return Err(error),
    }
    Err(DispatchError::NotImplemented)
}

async fn worktrees(pending: &mut Option<Context<'_>>, state: &State) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.worktree.list.request"
                | "workspace.worktree.create.request"
                | "workspace.worktree.archive.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

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
        Ok(reply) => context
            .workspace(reply.value, reply.event)
            .map_err(Into::into),
        Err(error) => context.respond(Err(error)).map_err(Into::into),
    }
}

async fn checkout(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "checkout.status.get.request"
                | "checkout.refresh.request"
                | "checkout.diff.get.request"
                | "checkout.diff.subscribe.request"
                | "checkout.diff.unsubscribe.request"
                | "checkout.commits.list.request"
                | "checkout.commits.file_diff.request"
                | "checkout.branch.validate.request"
                | "checkout.branch.suggestions.request"
                | "checkout.branch.switch.request"
                | "checkout.rename_branch.request"
                | "checkout.commit.request"
                | "checkout.merge.request"
                | "checkout.merge_from_base.request"
                | "checkout.reset_workspace.request"
                | "checkout.pull.request"
                | "checkout.push.request"
                | "checkout.discard_changes.request"
                | "checkout.stash.save.request"
                | "checkout.stash.pop.request"
                | "checkout.stash.list.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if let Err(error) =
        metadata::fill(state, &context.request.method, &mut context.request.params).await
    {
        return context.respond(Err(error)).map_err(Into::into);
    }

    if context.request.method == "checkout.diff.unsubscribe.request" {
        let request = serde_json::from_value::<
            crate::protocol::checkout::CheckoutDiffUnsubscribeRequest,
        >(std::mem::take(&mut context.request.params));
        let request = match request {
            Ok(request) if valid_id(&request.subscription_id) => request,
            _ => {
                return context
                    .respond(Err(ErrorCode::InvalidMessage))
                    .map_err(Into::into);
            }
        };
        if connection.diffs.remove(&request.subscription_id).is_none() {
            return context
                .respond(Err(ErrorCode::SubscriptionNotFound))
                .map_err(Into::into);
        }
        return context
            .respond(Ok(json!({"subscriptionId":request.subscription_id})))
            .map_err(Into::into);
    }
    if context.request.method == "checkout.diff.subscribe.request" {
        let replacement = context
            .request
            .params
            .get("subscriptionId")
            .and_then(Value::as_str)
            .is_some_and(|id| connection.diffs.contains_key(id));
        if context.available_subscriptions == 0 && !replacement {
            return context
                .respond(Err(ErrorCode::ResourceExhausted))
                .map_err(Into::into);
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
        Err(error) => context.respond(Err(error)).map_err(Into::into),
    }
}
