use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::{LocalAddress, Shared};

pub(super) const PATH: &str = "/api/terminal-activity";

pub(super) fn compose(
    runtime: &Arc<model::Runtime>,
    terminals: Option<terminal::service::Terminals>,
    address: SocketAddr,
    events: &model::session::SessionEvents,
) -> Arc<terminal::dispatch::State> {
    Arc::new(terminal::dispatch::State {
        runtime: runtime.clone(),
        terminals: terminals.map(|mut terminals| {
            terminals.set_activity_url(url(address));
            terminals.set_session_events(events.clone(), runtime.info().server_id);
            crate::shared_service(terminals)
        }),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    terminal_id: String,
    token: String,
    state: terminal::activity::ReportState,
}

pub(super) fn url(address: SocketAddr) -> String {
    let ip = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}{PATH}", SocketAddr::new(ip, address.port()))
}

pub(super) async fn report(State(state): State<Arc<Shared>>, request: Request) -> Response {
    if !request
        .extensions()
        .get::<ConnectInfo<LocalAddress>>()
        .is_some_and(|address| address.0.is_loopback_peer())
    {
        return failure(StatusCode::FORBIDDEN, "Forbidden");
    }
    let Ok(bytes) = axum::body::to_bytes(request.into_body(), 4096).await else {
        return failure(StatusCode::BAD_REQUEST, "Invalid terminal activity report");
    };
    let Ok(report) = serde_json::from_slice::<Report>(&bytes) else {
        return failure(StatusCode::BAD_REQUEST, "Invalid terminal activity report");
    };
    if report.terminal_id.is_empty() || report.token.is_empty() {
        return failure(StatusCode::BAD_REQUEST, "Invalid terminal activity report");
    }
    match terminal::dispatch::report_activity(
        &state.terminal,
        report.terminal_id,
        report.token.into(),
        report.state,
    )
    .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) | Err(model::ErrorCode::NotImplemented) => {
            failure(StatusCode::FORBIDDEN, "Forbidden")
        }
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update terminal activity",
        ),
    }
}

fn failure(status: StatusCode, error: &str) -> Response {
    (status, Json(serde_json::json!({"error":error}))).into_response()
}

#[cfg(test)]
mod tests;
