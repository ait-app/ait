//! Request branches tried in order by this crate; each owns recognition and consumption.

use model::{Context, DispatchError, ErrorCode};

use super::{Completion, State};
use crate::connection::Connection;

/// Try the push request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn push(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(context) = pending
        .take_if(|context| matches!(context.request.method.as_str(), "push.unregister.request"))
    else {
        return Err(DispatchError::NotImplemented);
    };
    crate::connection::push::unregister(context, state, connection)
        .await
        .map_err(Into::into)
}

/// Try the editor request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) fn editor(
    pending: &mut Option<Context<'_>>,
    _state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "editor.available.list.request" | "editor.open.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = crate::rpc::editor::execute(
        &context.request.method,
        std::mem::take(&mut context.request.params),
    );
    context.respond(result).map_err(Into::into)
}

/// Try the creation request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn creation(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "creation.subscribe.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    crate::connection::creation::dispatch(context, state, connection)
        .await
        .map_err(Into::into)
}

/// Try the session request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) fn session(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "session.events.set_subscription.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    crate::connection::session::subscribe(context, state, connection).map_err(Into::into)
}

/// Try the directory request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn directory(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "project.add.request"
                | "project.create_directory.request"
                | "project.list.request"
                | "project.rename.request"
                | "project.remove.request"
                | "workspace.open.request"
                | "workspace.create.request"
                | "workspace.list.request"
                | "workspace.archive.request"
                | "workspace.title.set.request"
                | "workspace.pin.set.request"
                | "project.config.read.request"
                | "project.config.write.request"
                | "project.icon.set.request"
                | "project.icon.get.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if context.request.method == "workspace.create.request" {
        return crate::connection::creation::create(context, state)
            .await
            .map_err(Into::into);
    }
    if context.request.method == "workspace.list.request"
        && context
            .request
            .params
            .get("subscribe")
            .is_some_and(|value| !value.is_null())
    {
        return crate::connection::directory::subscribe(context, state, connection)
            .await
            .map_err(Into::into);
    }
    context
        .rpc(
            state.directory.clone(),
            ErrorCode::RegistryIo,
            crate::rpc::directory::execute,
        )
        .await
        .map_err(Into::into)
}

/// Try the daemon request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn daemon(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<Option<Completion>, DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "daemon.get_status.request"
                | "daemon.get_pairing_offer.request"
                | "daemon.config.reload.request"
                | "daemon.update.request"
                | "diagnostics.request"
                | "daemon.config.get.request"
                | "daemon.config.set.request"
                | "server.restart.request"
                | "server.shutdown.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if matches!(
        context.request.method.as_str(),
        "daemon.get_status.request" | "diagnostics.request"
    ) {
        if !context.request.params.is_object() {
            context.respond(Err(ErrorCode::InvalidMessage))?;
            return Ok(None);
        }
        return Ok(Some(Completion::DaemonSnapshot {
            request_id: context.request.id,
            method: context.request.method,
            params: context.request.params,
        }));
    }
    let params = std::mem::take(&mut context.request.params);
    let result = crate::connection::daemon::dispatch(&context.request.method, params, state).await;
    context.respond(result)?;
    Ok(None)
}

/// Try the automation request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn automation(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.setup.status.request"
                | "workspace.setup.run.request"
                | "workspace.script.list.request"
                | "workspace.script.start.request"
                | "workspace.script.stop.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    context
        .rpc(
            state.workspace_automation.clone(),
            ErrorCode::RegistryIo,
            |service, method, params| {
                crate::rpc::workspace_automation::execute(service, method, params)
            },
        )
        .await
        .map_err(Into::into)
}

/// Try the workspace state request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn workspace_state(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.clear_attention.request" | "workspace.mark_unread.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = context
        .call(
            state.workspace_state.clone(),
            ErrorCode::RegistryIo,
            crate::rpc::workspace_state::execute,
        )
        .await;
    match result {
        Ok(reply) => context.workspace(reply.value, reply.event),
        Err(error) => context.respond(Err(error)),
    }
    .map_err(Into::into)
}
