//! Stateless Streamable HTTP MCP adapter for the daemon's installed capabilities.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use bot::mcp::{Caller, CallerKind};
use domain::bot::BOT_SCOPE_LABEL;
use serde_json::{Value, json};
use tower_http::timeout::TimeoutLayer;

use crate::connection::rpc;
use crate::{ApiError, Shared, auth};

mod catalog;
mod scope;
#[cfg(unix)]
mod socket;
mod tools;
mod validation;

use scope::{Failure, Scope};

/// Canonical agent orchestration endpoint on the existing daemon listener.
pub(super) const PATH: &str = "/mcp/agents";
const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest a detached call (a scoped `create_agent`) may run in all once its caller stopped
/// waiting; it holds its call permit until then.
const DETACHED_TIMEOUT: Duration = Duration::from_secs(300);

/// Authenticated identity of one MCP request, inserted into its extensions by the guard.
#[derive(Debug, Clone)]
pub(super) enum Principal {
    /// The daemon token or the shared Agent MCP token.
    Host,
    /// A Bot session or Bot-scoped Agent credential.
    Scoped(Caller),
}

/// Authenticate an MCP request with the daemon token, the shared Agent MCP token or a scoped
/// credential issued by the installed Bot service, in that order.
/// # Errors
/// Returns 400 for malformed authorization headers and 401 for unknown credentials.
pub(super) fn authenticate(state: &Shared, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let presented = auth::bearer(headers)?;
    if auth::matches(presented, &state.token)
        || state
            .agent_mcp_token
            .as_ref()
            .is_some_and(|token| auth::matches(presented, token))
    {
        return Ok(Principal::Host);
    }
    state
        .bot
        .bots
        .as_ref()
        .and_then(|bots| bots.scopes().resolve(presented))
        .map(Principal::Scoped)
        .ok_or(ApiError(StatusCode::UNAUTHORIZED))
}

/// Read the current grant of a scoped `principal` once for the whole request.
/// # Errors
/// Returns 401 when the Bot service that issued the credential is not installed.
fn scope(state: &Shared, principal: Principal) -> Result<Option<Scope>, ApiError> {
    match principal {
        Principal::Host => Ok(None),
        Principal::Scoped(caller) => {
            let grant = state
                .bot
                .bots
                .as_ref()
                .map(|bots| bots.scopes().grant(&caller.root))
                .ok_or(ApiError(StatusCode::UNAUTHORIZED))?;
            Ok(Some(Scope { caller, grant }))
        }
    }
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
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    enabled(&state).await?;
    validate_headers(&headers)?;
    let scope = scope(&state, principal)?;
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
            json!({"tools":catalog::granted(&state, scope.as_ref()).map(|tool| tool.descriptor(scope.as_ref())).collect::<Vec<_>>()}),
        ),
        "tools/list" => Err((-32602, "This catalog has no continuation cursor")),
        "tools/call" => call(state, scope, &headers, params).await,
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
    scope: Option<Scope>,
    headers: &HeaderMap,
    params: Value,
) -> Result<Value, (i32, &'static str)> {
    let name = params["name"]
        .as_str()
        .ok_or((-32602, "Tool name is required"))?;
    let tool = catalog::granted(&state, scope.as_ref())
        .find(|tool| tool.name == name)
        .ok_or((-32602, "Unknown or unavailable tool"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    validation::arguments(name, &tool.schema(scope.as_ref()), &arguments)
        .map_err(|message| (-32602, message))?;
    let header = auth::single_header(headers, "x-ait-agent-id")
        .map_err(|_| (-32602, "Invalid caller header"))?;
    let caller = claimed(scope.as_ref(), header)?.map(str::to_owned);
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
    // A scoped `create_agent` opens or joins a Bot ticket and must finish its bookkeeping
    // (assign the Agent, or delete a ticket whose Agent was not created) even when the caller
    // stops waiting (ADR-274 section 6).
    let detach = (scope.is_some() && tool.name == "create_agent").then_some(DETACHED_TIMEOUT);
    let task_state = state.clone();
    state.tasks.spawn(async move {
        let tracker = task_state.tasks.clone();
        let invocation_state = task_state.clone();
        let invocation = async move {
            invoke(
                &invocation_state,
                tool,
                arguments,
                scope.as_ref(),
                caller.as_deref(),
            )
            .await
        };
        let result = tokio::select! {
            () = task_state.cancellation.cancelled() => Ok(tool_result(Err(model::ErrorCode::ServerDraining.into()))),
            result = bounded(&tracker, invocation, permit, detach, CALL_TIMEOUT) => {
                result.unwrap_or_else(|| Ok(tool_result(Err(model::ErrorCode::ResourceExhausted.into()))))
            }
        };
        let _ = sender.send(result);
    });
    drop(admission);
    receiver
        .await
        .map_err(|_| (-32603, "MCP execution failed"))?
}

/// Await `work` for at most `limit`, holding `held` (the call's permit) while it runs; `None`
/// when the wait ran out. With `detach`, `work` runs as its own task on `tasks` together with
/// `held`, so running out stops only the wait: the work still completes, keeping `held` until
/// it ends, unless it outlives `detach` in all, when it is dropped. A detached task that
/// panicked also yields `None`.
async fn bounded<F, H>(
    tasks: &tokio_util::task::TaskTracker,
    work: F,
    held: H,
    detach: Option<Duration>,
    limit: Duration,
) -> Option<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
    H: Send + 'static,
{
    let Some(detached) = detach else {
        let _held = held;
        return tokio::time::timeout(limit, work).await.ok();
    };
    let task = tasks.spawn(async move {
        let _held = held;
        let finished = tokio::time::timeout(detached, work).await;
        finished
            .inspect_err(|_| tracing::warn!("detached MCP call ran out of time and was dropped"))
            .ok()
    });
    let joined = tokio::time::timeout(limit, task).await.ok()?;
    joined
        .inspect_err(|error| tracing::error!(?error, "detached MCP call failed"))
        .ok()
        .flatten()
}

/// Resolve the `caller` Agent's context and execute `tool` as a tool result.
/// # Errors
/// Rejects host credentials naming a Bot-scoped Agent, which must use its own credential.
async fn invoke(
    state: &Shared,
    tool: &catalog::Tool,
    arguments: Value,
    scope: Option<&Scope>,
    caller: Option<&str>,
) -> Result<Value, (i32, &'static str)> {
    let context = match caller {
        Some(id) => match context(state, id).await {
            Ok(agent) => Some(agent),
            Err(error) => return Ok(tool_result(Err(error.into()))),
        },
        None => None,
    };
    if scope.is_none()
        && context
            .as_ref()
            .is_some_and(|agent| agent["labels"].get(BOT_SCOPE_LABEL).is_some())
    {
        return Err((-32602, "Bot-scoped agents must use their own credential"));
    }
    let result = tools::execute(state, tool, arguments, scope, context.as_ref()).await;
    Ok(tool_result(result))
}

/// Return the Agent whose record supplies the caller context, given the `header` identity.
///
/// A Bot session has no public Agent record and gets no context; a subtree Agent always acts as
/// itself; the host may name any Agent.
/// # Errors
/// Rejects a header naming another identity than the scoped credential.
fn claimed<'a>(
    scope: Option<&'a Scope>,
    header: Option<&'a str>,
) -> Result<Option<&'a str>, (i32, &'static str)> {
    let Some(scope) = scope else {
        return Ok(header);
    };
    let (own, context) = match &scope.caller.kind {
        CallerKind::Bot { session_id, .. } => (session_id.as_str(), None),
        CallerKind::Subtree { agent_id, .. } => (agent_id.as_str(), Some(agent_id.as_str())),
    };
    if header.is_some_and(|header| header != own) {
        return Err((-32602, "Caller does not match credential"));
    }
    Ok(context)
}

/// Resolve public Agent `id` as caller context; hidden internal records are not found.
/// # Errors
/// Returns `InvalidMessage` for unknown Agents and propagates lookup failures.
async fn context(state: &Shared, id: &str) -> Result<Value, model::ErrorCode> {
    let mut value = rpc::execute(
        state,
        "agent.get.request",
        json!({"agentId":id,"publicOnly":true}),
    )
    .await?;
    match value.get_mut("agent").map(Value::take) {
        Some(agent) if agent.is_object() => Ok(agent),
        _ => Err(model::ErrorCode::InvalidMessage),
    }
}

fn tool_result(result: Result<Value, Failure>) -> Value {
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
