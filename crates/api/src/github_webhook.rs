//! Signature-authenticated GitHub App webhook admission.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use domain::ChannelId;
use model::ErrorCode;
use tower_http::timeout::TimeoutLayer;

use crate::{ApiError, Shared, auth};

const PREFIX: &str = "/v1/bots/github/";
const SUFFIX: &str = "/webhook";

pub(super) fn router() -> Router<Arc<Shared>> {
    Router::new()
        .route("/v1/bots/github/{channel_id}/webhook", post(handle))
        .layer(DefaultBodyLimit::max(model::server::MAX_MESSAGE_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(50),
        ))
}

pub(super) fn is_request(method: &Method, path: &str) -> bool {
    if method != Method::POST {
        return false;
    }
    path.strip_prefix(PREFIX)
        .and_then(|path| path.strip_suffix(SUFFIX))
        .and_then(|channel| ChannelId::from_str(channel).ok())
        .is_some()
}

async fn handle(
    State(state): State<Arc<Shared>>,
    Path(channel_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    let channel_id =
        ChannelId::from_str(&channel_id).map_err(|_| ApiError(StatusCode::NOT_FOUND))?;
    let event = required_header(&headers, "x-github-event")?;
    let delivery = required_header(&headers, "x-github-delivery")?;
    let signature = required_header(&headers, "x-hub-signature-256")?;
    let bots = state
        .bot
        .bots
        .as_ref()
        .ok_or(ApiError(StatusCode::NOT_FOUND))?;
    let result = bots
        .github_webhook(channel_id, event, delivery, signature, &body)
        .await
        .map_err(map_error)?;
    Ok((StatusCode::ACCEPTED, Json(result)).into_response())
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, ApiError> {
    auth::single_header(headers, name)?.ok_or(ApiError(StatusCode::BAD_REQUEST))
}

fn map_error(error: ErrorCode) -> ApiError {
    ApiError(match error {
        ErrorCode::BotNotFound => StatusCode::NOT_FOUND,
        ErrorCode::InvalidMessage => StatusCode::UNAUTHORIZED,
        ErrorCode::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::ServerDraining => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    })
}

#[cfg(test)]
mod tests;
