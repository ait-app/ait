//! Request branches tried in order by this crate; each owns recognition and consumption.

use model::{Context, DispatchError, ErrorCode};

use super::{Completion, State};

/// Try the agents request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn agents(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.configure"
                | "agent.get"
                | "agent.list"
                | "agent.default.get"
                | "agent.default.set"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    context
        .rpc(
            state.agents.clone(),
            ErrorCode::AgentIo,
            crate::rpc::agents::execute,
        )
        .await
        .map_err(Into::into)
}

/// Try the runtime request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn runtime(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut crate::connection::Connection,
) -> Result<Option<Completion>, DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.list.request"
                | "agent.history.get.request"
                | "agent.get.request"
                | "agent.update.request"
                | "agent.archive.request"
                | "agent.delete.request"
                | "agent.detach.request"
                | "agent.attention.clear.request"
                | "agent.items.close.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if context.request.method == "agent.list.request"
        && context
            .request
            .params
            .get("subscribe")
            .is_some_and(|value| !value.is_null())
    {
        crate::connection::directory::subscribe(context, state, connection).await?;
        return Ok(None);
    }
    let params = std::mem::take(&mut context.request.params);
    match super::agent_runtime::dispatch(&context.request.method, params, state).await {
        Ok(reply) if !reply.terminals.is_empty() => Ok(Some(Completion::CloseTerminals {
            request_id: context.request.id,
            value: reply.value,
            terminal_ids: reply.terminals,
        })),
        Ok(reply) => {
            context.respond(Ok(reply.value))?;
            Ok(None)
        }
        Err(error) => {
            context.respond(Err(error))?;
            Ok(None)
        }
    }
}

/// Try the execution request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn execution(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.create.request"
                | "agent.resume.request"
                | "agent.message.send.request"
                | "agent.cancel.request"
                | "agent.finish.wait.request"
                | "agent.model.set.request"
                | "agent.thinking.set.request"
                | "agent.config.apply.request"
                | "provider.sessions.recent.list.request"
                | "agent.import.request"
                | "agent.refresh.request"
                | "agent.fork_context.request"
                | "agent.rewind.request"
                | "agent.commands.list.request"
                | "agent.mode.set.request"
                | "agent.feature.set.request"
                | "agent.permission.resolve.request"
                | "agent.provider_subagents.list.request"
                | "agent.provider_subagents.timeline.get.request"
                | "provider.diagnostic.request"
                | "provider.usage.list.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if context.request.method == "agent.create.request" {
        return crate::connection::Connection::create(context, state)
            .await
            .map_err(Into::into);
    }
    if context.request.method == "agent.finish.wait.request" {
        return super::agent_execution::wait(
            context.request.id,
            context.request.params,
            state,
            context.outbound,
        )
        .map_err(Into::into);
    }
    let params = std::mem::take(&mut context.request.params);
    let result = super::agent_execution::dispatch(&context.request.method, params, state).await;
    context.respond(result).map_err(Into::into)
}

/// Try the timeline request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn timeline(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut crate::connection::Connection,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.timeline.get.request"
                | "agent.timeline.search.request"
                | "agent.timeline.list_prompts.request"
                | "agent.timeline.append.request"
                | "agent.timeline.set_subscription.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    if context.request.method == "agent.timeline.set_subscription.request" {
        return connection
            .subscribe(context, state)
            .await
            .map_err(Into::into);
    }
    if context.request.method == "agent.timeline.append.request" {
        let Some(plugin) = connection.plugin() else {
            return context
                .respond(Err(ErrorCode::UnsupportedCapability))
                .map_err(Into::into);
        };
        let payload = serde_json::json!({"request":context.request.params,"plugin":plugin});
        let result =
            super::agent_execution::dispatch("internal.timeline.append", payload, state).await;
        return context.respond(result).map_err(Into::into);
    }
    let params = std::mem::take(&mut context.request.params);
    let result = super::agent_execution::dispatch(&context.request.method, params, state).await;
    context.respond(result).map_err(Into::into)
}

/// Try the catalog request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) fn catalog(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "provider.available.list.request"
                | "provider.models.list.request"
                | "provider.modes.list.request"
                | "provider.features.list.request"
                | "provider.snapshot.get.request"
                | "provider.snapshot.refresh.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    super::catalog::request(context, state).map_err(Into::into)
}
