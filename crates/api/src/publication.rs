//! Shared machine authorization and complete disconnection over authenticated HTTP/RPC.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use host_link::control::{Authorization, AuthorizationMode};
use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode};
use serde_json::{Value, json};

use crate::{ApiError, Shared};

pub(super) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("publication.status.request"),
    MethodSpec::request("publication.authorize.request"),
    MethodSpec::request("publication.stop.request"),
];

async fn status(state: &Shared) -> Value {
    json!({
        "serverId": state.info.server_id,
        "instanceId": state.info.instance_id,
        "platform": std::env::consts::OS,
        "management": state.relay_management(),
        "hub": {
            "status": state.relay.hub().status().await,
        },
        "relay": state.relay.status().await,
    })
}

pub(super) async fn http_status(State(state): State<std::sync::Arc<Shared>>) -> Json<Value> {
    Json(status(&state).await)
}

pub(super) async fn http_authorize(
    State(state): State<std::sync::Arc<Shared>>,
    input: Result<Json<Authorization>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(authorization) = input.map_err(|_| ApiError(StatusCode::BAD_REQUEST))?;
    let control = state
        .publication_control
        .as_ref()
        .ok_or(ApiError(StatusCode::CONFLICT))?;
    control
        .authorize(authorization, AuthorizationMode::Explicit)
        .await
        .map_err(http_error)?;
    Ok(StatusCode::ACCEPTED)
}

pub(super) async fn http_disconnect(
    State(state): State<std::sync::Arc<Shared>>,
) -> Result<StatusCode, ApiError> {
    let control = state
        .publication_control
        .as_ref()
        .ok_or(ApiError(StatusCode::CONFLICT))?;
    control.disconnect().await.map_err(http_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn http_error(error: host_link::Error) -> ApiError {
    ApiError(match error {
        host_link::Error::Protocol => StatusCode::BAD_REQUEST,
        host_link::Error::Unauthorized => StatusCode::CONFLICT,
        host_link::Error::Storage => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    })
}

pub(super) async fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), DispatchError> {
    let Some(context) = context.take_if(|context| {
        METHODS
            .iter()
            .any(|method| method.name == context.request.method)
            || (context.request.method == "relay.stop.request"
                && state.publication_control.is_some())
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = execute(&context.request, state).await;
    context.respond(result).map_err(Into::into)
}

async fn execute(request: &model::Request, state: &Shared) -> Result<Value, ErrorCode> {
    let control = state
        .publication_control
        .as_ref()
        .ok_or(ErrorCode::RelayManaged)?;
    let result = match request.method.as_str() {
        "publication.authorize.request" => {
            let authorization = serde_json::from_value(request.params["authorization"].clone())
                .map_err(|_| ErrorCode::InvalidMessage)?;
            let mode = request
                .params
                .get("mode")
                .map_or(Ok(AuthorizationMode::Explicit), |mode| {
                    serde_json::from_value(mode.clone()).map_err(|_| ErrorCode::InvalidMessage)
                })?;
            control.authorize(authorization, mode).await
        }
        "publication.stop.request" | "relay.stop.request" => control.disconnect().await,
        "publication.status.request" => Ok(()),
        _ => return Err(ErrorCode::MethodNotFound),
    };
    result.map_err(|error| match error {
        host_link::Error::Protocol => ErrorCode::InvalidMessage,
        _ => ErrorCode::AgentIo,
    })?;
    if request.method == "relay.stop.request" {
        return Ok(json!({
            "serverId": state.info.server_id,
            "instanceId": state.info.instance_id,
            "platform": std::env::consts::OS,
            "status": state.relay.status().await,
            "management": state.relay_management(),
        }));
    }
    Ok(status(state).await)
}
