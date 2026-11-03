//! Stateless Streamable HTTP MCP adapter for the daemon's installed capabilities.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tower_http::timeout::TimeoutLayer;

use crate::{ApiError, Shared, auth};

mod catalog;
#[cfg(unix)]
mod socket;
mod tools;
mod validation;

/// Canonical agent orchestration endpoint on the existing daemon listener.
pub(super) const PATH: &str = "/mcp/agents";
const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn authenticate(state: &Shared, headers: &HeaderMap) -> Result<(), ApiError> {
    auth::authenticate(headers, &state.token).or_else(|error| {
        state
            .agent_mcp_token
            .as_ref()
            .map_or(Err(error), |token| auth::authenticate(headers, token))
    })
}

/// Return stateless MCP routes; the parent API supplies shared state and authentication.
pub(super) fn router() -> Router<Arc<Shared>> {
    Router::new()
        .route(
            PATH,
            post(handle)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(model::server::MAX_MESSAGE_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(65),
        ))
}

async fn enabled(state: &Shared) -> Result<(), ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    if let Some(daemon) = &state.metadata.daemon {
        let config = state
            .runtime
            .run_queued(Some(daemon.clone()), model::ErrorCode::DaemonIo, |daemon| {
                daemon.get_config().map_err(|_| model::ErrorCode::DaemonIo)
            })
            .await
            .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE))?;
        if config["mcp"]["enabled"] == false {
            return Err(ApiError(StatusCode::NOT_FOUND));
        }
    }
    Ok(())
}

async fn method_not_allowed(State(state): State<Arc<Shared>>) -> Result<Response, ApiError> {
    enabled(&state).await?;
    Ok((
        StatusCode::METHOD_NOT_ALLOWED,
        [("allow", "POST")],
        Json(error(&Value::Null, -32000, "Method not allowed")),
    )
        .into_response())
}

async fn handle(
    State(state): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    enabled(&state).await?;
    validate_headers(&headers)?;
    let message: Value = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(_) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(error(&Value::Null, -32700, "Parse error")),
            )
                .into_response());
        }
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    if message["jsonrpc"] != "2.0"
        || !message["method"].is_string()
        || !valid_id(&id, message.get("id").is_some())
        || message
            .get("params")
            .is_some_and(|params| !params.is_object())
    {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(error(&Value::Null, -32600, "Invalid Request")),
        )
            .into_response());
    }
    let method = message["method"].as_str().expect("validated method string");
    if message.get("id").is_none() {
        // Stateless clients acknowledge initialization independently on every request.
        return Ok(if method.starts_with("notifications/") {
            StatusCode::ACCEPTED.into_response()
        } else {
            (
                StatusCode::BAD_REQUEST,
                Json(error(&Value::Null, -32600, "Invalid Request")),
            )
                .into_response()
        });
    }
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => initialize(&params),
        "ping" => Ok(json!({})),
        "tools/list" if params.get("cursor").is_none() => Ok(
            json!({"tools":catalog::installed(&state).map(catalog::Tool::descriptor).collect::<Vec<_>>()}),
        ),
        "tools/list" => Err((-32602, "This catalog has no continuation cursor")),
        "tools/call" => call(state, &headers, params).await,
        _ => Err((-32601, "Method not found")),
    };
    Ok(Json(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err((code, message)) => error(&id, code, message),
    })
    .into_response())
}

fn validate_headers(headers: &HeaderMap) -> Result<(), ApiError> {
    let content_type =
        auth::single_header(headers, "content-type")?.and_then(|value| value.split(';').next());
    if !content_type.is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json")) {
        return Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    }
    if !accepts(headers, "application/json") || !accepts(headers, "text/event-stream") {
        return Err(ApiError(StatusCode::NOT_ACCEPTABLE));
    }
    if auth::single_header(headers, "mcp-protocol-version")?
        .is_some_and(|version| !VERSIONS.contains(&version))
    {
        return Err(ApiError(StatusCode::BAD_REQUEST));
    }
    Ok(())
}

fn accepts(headers: &HeaderMap, expected: &str) -> bool {
    headers.get_all("accept").iter().any(|header| {
        header.to_str().is_ok_and(|header| {
            header.split(',').any(|range| {
                let mut parts = range.split(';');
                parts
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(expected))
                    && parts.all(|parameter| {
                        parameter.split_once('=').is_none_or(|(name, value)| {
                            !name.trim().eq_ignore_ascii_case("q")
                                || value
                                    .trim()
                                    .parse::<f64>()
                                    .is_ok_and(|quality| quality > 0.0 && quality <= 1.0)
                        })
                    })
            })
        })
    })
}

fn valid_id(id: &Value, present: bool) -> bool {
    !present || id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}

fn initialize(params: &Value) -> Result<Value, (i32, &'static str)> {
    let Some(version) = params["protocolVersion"].as_str() else {
        return Err((-32602, "protocolVersion is required"));
    };
    if !params["capabilities"].is_object()
        || !params["clientInfo"]["name"].is_string()
        || !params["clientInfo"]["version"].is_string()
    {
        return Err((-32602, "capabilities and clientInfo are required"));
    }
    Ok(json!({
        "protocolVersion":if VERSIONS.contains(&version) { version } else { "2025-11-25" },
        "capabilities":{"tools":{}},
        "serverInfo":{"name":"ait-daemon","version":model::BUILD_VERSION},
        "instructions":"Use list_providers/list_models to select a provider/model. Create an isolated workspace when needed. Agent creation and prompts return after admission; poll get_agent_status/get_agent_activity or use wait_for_agent. Pass X-Ait-Agent-Id for caller workspace and parentage inheritance."
    }))
}

async fn call(
    state: Arc<Shared>,
    headers: &HeaderMap,
    params: Value,
) -> Result<Value, (i32, &'static str)> {
    let name = params["name"]
        .as_str()
        .ok_or((-32602, "Tool name is required"))?;
    let tool = catalog::installed(&state)
        .find(|tool| tool.name == name)
        .ok_or((-32602, "Unknown or unavailable tool"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !validation::matches(&tool.input_schema, &arguments) {
        return Err((-32602, "Arguments do not match the tool input schema"));
    }
    let caller = auth::single_header(headers, "x-ait-agent-id")
        .map_err(|_| (-32602, "Invalid caller header"))?
        .map(str::to_owned);
    let permit = state
        .connections
        .clone()
        .try_acquire_owned()
        .map_err(|_| (-32000, "MCP request budget exhausted"))?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let admission = state
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.cancellation.is_cancelled() {
        return Err((-32000, "Server is draining"));
    }
    let task_state = state.clone();
    state.tasks.spawn(async move {
        let _permit = permit;
        let result = tokio::select! {
            () = task_state.cancellation.cancelled() => Err(model::ErrorCode::ServerDraining),
            result = tokio::time::timeout(CALL_TIMEOUT, tools::execute(&task_state, tool, arguments, caller.as_deref())) => {
                result.unwrap_or(Err(model::ErrorCode::ResourceExhausted))
            }
        };
        let _ = sender.send(tool_result(result));
    });
    drop(admission);
    receiver.await.map_err(|_| (-32603, "MCP execution failed"))
}

fn tool_result(result: Result<Value, model::ErrorCode>) -> Value {
    match result {
        Ok(value) => {
            let failed = value.get("error").is_some_and(|error| !error.is_null())
                || value["success"] == false
                || value["accepted"] == false;
            json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":failed})
        }
        Err(error) => json!({"content":[{"type":"text","text":error.message()}],"isError":true}),
    }
}

fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests;
