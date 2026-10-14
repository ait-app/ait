//! Request branches tried in order by this crate; each owns recognition and consumption.

use model::{Context, DispatchError, ErrorCode};

use super::State;
use crate::connection::Connection;

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
        super::summary::fill(state, &context.request.method, &mut context.request.params).await
    {
        return context.respond(Err(error)).map_err(Into::into);
    }
    context
        .rpc(
            state.forge.clone(),
            ErrorCode::ProjectIo,
            |service, method, params| crate::forge::rpc::execute(service, method, params),
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
            crate::files::connection::FileRequest {
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
