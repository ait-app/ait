use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use chrono::Utc;
use host_link::{Binding, Machine};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::http::HttpCenter;

pub(super) struct Fixture {
    pub center: HttpCenter,
    pub seen: Arc<Mutex<Seen>>,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct Seen {
    pub calls: Vec<(String, Value, Option<String>)>,
    pub responses: VecDeque<(StatusCode, Value)>,
}

impl Fixture {
    pub async fn new(responses: Vec<(StatusCode, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Seen {
            responses: responses.into(),
            ..Seen::default()
        }));
        let router = axum::Router::new()
            .fallback(handler)
            .with_state(seen.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            center: HttpCenter::test_center(format!("http://{address}")),
            seen,
            task,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handler(
    State(state): State<Arc<Mutex<Seen>>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut seen = state.lock().unwrap();
    seen.calls.push((
        uri.path().to_owned(),
        body,
        headers
            .get("authorization")
            .map(|v| v.to_str().unwrap().to_owned()),
    ));
    let (status, body) = seen
        .responses
        .pop_front()
        .unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, json!({})));
    (status, Json(body))
}

pub(super) fn declaration() -> Machine {
    Machine {
        server_id: Uuid::new_v4(),
        display_name: "build-linux".into(),
        platform: "linux".into(),
        app_version: "test".into(),
    }
}

pub(super) fn response(machine: &Machine) -> Value {
    json!({"token_type":"Bearer", "access_token":"jwt.device.secret", "expires_in":900,
        "access_expires_at":Utc::now() + chrono::Duration::minutes(15),
        "refresh_token":format!("ait_refresh_{}", "a".repeat(64)),
        "refresh_expires_at":Utc::now() + chrono::Duration::days(30),
        "node_id":Uuid::new_v4(),"host_id":Uuid::new_v4(),"server_id":machine.server_id,"grant_id":Uuid::new_v4()})
}

pub(super) fn binding(value: &Value) -> Binding {
    serde_json::from_value(json!({"node_id":value["node_id"],"host_id":value["host_id"],"server_id":value["server_id"],"grant_id":value["grant_id"]})).unwrap()
}
