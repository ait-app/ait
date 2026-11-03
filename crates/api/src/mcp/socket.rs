//! MCP-only HTTP routing for a host-owned Unix socket.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::trace::TraceLayer;

use crate::{Api, ApiError, Shared, auth};

impl Api {
    /// Return MCP-only HTTP routes for a Unix socket, sharing tools and shutdown state.
    /// Clients use `http://localhost/mcp/agents` and either accepted MCP Bearer credential.
    /// Other routes return 404; invalid source headers, credentials or URL queries are rejected.
    pub fn mcp_socket_router(&self) -> Router {
        super::router()
            .fallback(|| async { ApiError(StatusCode::NOT_FOUND) })
            .layer(middleware::from_fn_with_state(self.shared.clone(), guard))
            .layer(TraceLayer::new_for_http().make_span_with(
                |request: &Request| tracing::info_span!("server_mcp_socket", method = %request.method()),
            ))
            .with_state(self.shared.clone())
    }
}

async fn guard(
    State(state): State<Arc<Shared>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    auth::validate_source(request.headers(), &["localhost"], &state.browser_auth)?;
    super::authenticate(&state, request.headers())?;
    if request.uri().query().is_some() {
        return Err(ApiError(StatusCode::BAD_REQUEST));
    }
    Ok(next.run(request).await)
}
