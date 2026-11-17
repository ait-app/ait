//! Typed saved-Timeline exports; the API only finishes existing download-grant composition.

use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode};

use crate::ports::timeline_export::{CancelRequest, Request};

use super::{Completion, State};

pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("agent.timeline.export.request"),
    MethodSpec::request("agent.timeline.export.cancel.request"),
];

pub(super) async fn dispatch(
    pending: &mut Option<Context<'_>>,
    state: &State,
) -> Result<Option<Completion>, DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "agent.timeline.export.request" | "agent.timeline.export.cancel.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let params = std::mem::take(&mut context.request.params);
    let result = if context.request.method == "agent.timeline.export.cancel.request" {
        serde_json::from_value::<CancelRequest>(params)
            .map(|request| Completion::ReleaseTimelineExport {
                request_id: context.request.id.clone(),
                agent_id: request.agent_id,
                token: request.token,
            })
            .map_err(|_| ErrorCode::InvalidMessage)
    } else {
        match serde_json::from_value::<Request>(params) {
            Ok(request) => match state.agent_execution.as_ref() {
                Some(execution) => execution.export_timeline(request).await.map(|export| {
                    Completion::TimelineExport {
                        request_id: context.request.id.clone(),
                        export,
                    }
                }),
                None => Err(ErrorCode::NotImplemented),
            },
            Err(_) => Err(ErrorCode::InvalidMessage),
        }
    };
    match result {
        Ok(completion) => Ok(Some(completion)),
        Err(error) => {
            context.respond(Err(error))?;
            Ok(None)
        }
    }
}
