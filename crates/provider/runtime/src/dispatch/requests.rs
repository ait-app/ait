//! Request branches tried in order by this crate; each owns recognition and consumption.

use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode};

use super::{Completion, State};

/// Try the skills request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn skills(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let context = claim(pending, crate::skills::service::METHODS)?;
    context
        .rpc(
            state.skills.clone(),
            ErrorCode::RegistryIo,
            crate::skills::service::Skills::execute,
        )
        .await
        .map_err(Into::into)
}

/// Try the agents request branch and consume `pending` only on a match.
/// `state` supplies installed services; any `connection` argument owns subscriptions.
/// Returns the branch's completion after execution or response admission.
/// # Errors
/// Returns `NotImplemented` without consuming the request, or propagates delivery failure.
pub(super) async fn agents(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<(), DispatchError> {
    let context = claim(pending, crate::rpc::agents::METHODS)?;
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
    let mut context = claim(pending, crate::rpc::agent_runtime::METHODS)?;
    let mut sync_query = None;
    if context.request.method == "agent.list.request" {
        let params = &context.request.params;
        if let Some(page) = connection.directory_pages.resume(params) {
            context.respond(page)?;
            return Ok(None);
        }
        if !params["subscribe"].is_null() {
            crate::connection::directory::subscribe(context, state, connection).await?;
            return Ok(None);
        }
        if !params["sync"].is_null() {
            sync_query = Some(params.clone());
        }
    }
    let params = std::mem::take(&mut context.request.params);
    match super::agent_runtime::dispatch(&context.request.method, params, state).await {
        Ok(reply) if !reply.terminals.is_empty() => Ok(Some(Completion::CloseTerminals {
            request_id: context.request.id,
            value: reply.value,
            terminal_ids: reply.terminals,
        })),
        Ok(reply) => {
            let result = match sync_query {
                Some(params) => connection.directory_pages.first(&params, reply.value),
                None => Ok(reply.value),
            };
            context.respond(result)?;
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
    let mut context = claim(pending, super::agent_execution::METHODS)?;
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
    if context.request.method == "agent.context.compact.request" {
        let params = std::mem::take(&mut context.request.params);
        let result = super::agent_execution::compact(params, state).await;
        return context.respond(result).map_err(Into::into);
    }
    super::agent_execution::request(context, state).map_err(Into::into)
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
    let mut context = claim(pending, crate::rpc::timeline::METHODS)?;
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
        context.request.params =
            serde_json::json!({"request":context.request.params,"plugin":plugin});
        "internal.timeline.append".clone_into(&mut context.request.method);
    }
    if connection.knows_ticket_origins() {
        return super::agent_execution::request(context, state).map_err(Into::into);
    }
    super::agent_execution::request_projected(
        context,
        state,
        crate::timeline_projection::hide_ticket_origins,
    )
    .map_err(Into::into)
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
    let context = claim(pending, super::catalog::METHODS)?;
    super::catalog::request(context, state).map_err(Into::into)
}

/// Take `pending` when its method is one of `methods`, the owning component's declared set.
/// # Errors
/// Returns `NotImplemented` and leaves `pending` in place for any other method.
fn claim<'a>(
    pending: &mut Option<Context<'a>>,
    methods: &[MethodSpec],
) -> Result<Context<'a>, DispatchError> {
    pending
        .take_if(|context| {
            methods
                .iter()
                .any(|method| method.name == context.request.method)
        })
        .ok_or(DispatchError::NotImplemented)
}
