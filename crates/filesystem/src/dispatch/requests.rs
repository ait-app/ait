//! Request branches tried in order by this crate; each owns recognition and consumption.

use model::{Context, DispatchError, ErrorCode};

use super::State;
use crate::connection::Connection;

/// Try the skills request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn skills(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.skills.get_status.request"
                | "agent.skills.reconcile.request"
                | "agent.skills.uninstall.request"
                | "agent.skills.save_selection.request"
                | "agent.skills.import_legacy_selection.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    context
        .rpc(
            state.skills.clone(),
            ErrorCode::RegistryIo,
            crate::service::skills::Skills::execute,
        )
        .await
        .map_err(Into::into)
}

/// Try the forge request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn forge(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "forge.search.request"
                | "github.search.request"
                | "checkout.pr.create.request"
                | "checkout.pr.merge.request"
                | "checkout.pr.status.request"
                | "checkout.pr.timeline.request"
                | "checkout.forge.set_auto_merge.request"
                | "checkout.forge.get_check_details.request"
                | "checkout.github.set_auto_merge.request"
                | "checkout.github.get_check_details.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if let Err(error) =
        super::metadata::fill(state, &context.request.method, &mut context.request.params).await
    {
        return context.respond(Err(error)).map_err(Into::into);
    }
    context
        .rpc(
            state.forge.clone(),
            ErrorCode::ProjectIo,
            |service, method, params| crate::rpc::forge::execute(service, method, params),
        )
        .await
        .map_err(Into::into)
}

/// Try the github projects request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn github_projects(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.github.search_repositories.request" | "project.github.clone.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    context
        .rpc(
            state.github_projects.clone(),
            ErrorCode::RegistryIo,
            |service, method, params| crate::rpc::github_projects::execute(service, method, params),
        )
        .await
        .map_err(Into::into)
}

/// Try the files request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn files(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "directory.suggestions.request"
                | "fs.explorer.request"
                | "fs.file.subscribe.request"
                | "fs.file.unsubscribe.request"
                | "fs.file.write.request"
                | "fs.entry.create.request"
                | "fs.entry.rename.request"
                | "fs.entry.duplicate.request"
                | "fs.entry.delete.request"
                | "fs.file.download_token.request"
                | "file.upload.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
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
        .map_err(Into::into)
}

/// Try the recovery request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn recovery(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.recovery.inspect.request" | "workspace.recovery.restore.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
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
    .map_err(Into::into)
}
