use crate::{
    local::deepseek_harness::{DeepSeekHarnessClient, PROVIDER},
    ports::agent_session::{AgentSession, AgentSessionSpec, AgentTurnEvent},
};
use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use domain::agent_runtime::StoredAgentConfig;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::broadcast, task::JoinHandle};

#[derive(Clone)]
struct Host {
    cwd: String,
    requests: Arc<Mutex<Vec<Value>>>,
    frames: broadcast::Sender<Value>,
    records: Arc<Mutex<Vec<Value>>>,
    sessions: Arc<Mutex<Value>>,
    snapshot_fields: Arc<Mutex<Value>>,
}

pub(super) struct Fixture {
    directory: tempfile::TempDir,
    server: JoinHandle<()>,
    host: Host,
    pub(super) client: DeepSeekHarnessClient,
    pub(super) spec: AgentSessionSpec,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap().to_owned();
        let host = Host {
            cwd: cwd.clone(),
            requests: Arc::default(),
            records: Arc::new(Mutex::new(vec![
                json!({"type":"event","event":{"seq":0,"type":"permission/preset","time":1_700_000_000_000_i64,"data":{"preset":"workspace-write"}}}),
            ])),
            frames: broadcast::channel(128).0,
            sessions: Arc::new(Mutex::new(json!([]))),
            snapshot_fields: Arc::new(Mutex::new(json!({}))),
        };
        let app = Router::new()
            .route("/", get(auth))
            .route("/api/remote.mux", get(upgrade))
            .route("/api/{*endpoint}", post(rpc))
            .with_state(host.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/deepseek_harness_native.sh");
        let mut client = DeepSeekHarnessClient::new(program);
        client.environment = BTreeMap::from([("AIT_DSH_FIXTURE_PORT".into(), port.to_string())])
            .try_into()
            .unwrap();
        Self {
            directory,
            server,
            host,
            client,
            spec: AgentSessionSpec {
                provider: PROVIDER.into(),
                cwd,
                config: StoredAgentConfig::default(),
            },
        }
    }

    pub(super) fn seed_records(&self, records: Vec<Value>) {
        *self.host.records.lock().unwrap() = records;
    }
    pub(super) fn set_sessions(&self, rows: Value) {
        *self.host.sessions.lock().unwrap() = rows;
    }
    pub(super) fn set_snapshot_fields(&self, fields: Value) {
        *self.host.snapshot_fields.lock().unwrap() = fields;
    }
    pub(super) fn history(&self, seq: u64, kind: &str, data: Value) {
        let mut frame = json!({"type":"item","streamId":"history","value":{"type":"event","event":{"seq":seq,"type":kind,"time":1_700_000_000_000_u64+seq}}});
        frame["value"]["event"]["data"] = data;
        self.host
            .records
            .lock()
            .unwrap()
            .push(frame["value"].clone());
        self.send(frame);
    }
    pub(super) fn interaction(&self, frame: Value) {
        let mut envelope = json!({"type":"item","streamId":"events"});
        envelope["value"] = frame;
        self.send(envelope);
    }
    pub(super) fn send(&self, frame: Value) {
        self.host.frames.send(frame).unwrap();
    }
    pub(super) fn requests(&self, method: &str) -> Vec<Value> {
        self.host
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request["method"] == method)
            .map(|request| request["payload"]["args"].clone())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = self.directory.path();
    }
}

async fn auth() -> impl IntoResponse {
    (
        StatusCode::SEE_OTHER,
        [
            ("set-cookie", "dsh=fixture; HttpOnly; SameSite=Strict"),
            ("location", "/"),
        ],
    )
}
async fn upgrade(State(host): State<Host>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if headers.get("cookie").and_then(|value| value.to_str().ok()) != Some("dsh=fixture") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| stream(socket, host))
}
async fn stream(socket: WebSocket, host: Host) {
    let (mut writer, mut reader) = socket.split();
    let mut frames = host.frames.subscribe();
    loop {
        let value = tokio::select! {
            received=reader.next()=> {
                let Some(Ok(Message::Text(text)))=received else { break; };
                let request:Value=serde_json::from_str(&text).unwrap();
                if request["type"] == "cancel" { continue; }
                let value=match request["endpoint"].as_str() {
                    Some("$events")=>json!({"type":"ready","clientId":"client"}),
                    Some("session/follow")=>snapshot(&host),
                    Some("session/control")=>json!({"type":"baseline","value":{"projections":{}}}),
                    _=>break,
                };
                json!({"type":"item","streamId":request["streamId"],"value":value})
            },
            frame=frames.recv()=>{ let Ok(frame)=frame else { break; }; frame }
        };
        if writer
            .send(Message::Text(value.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}
async fn rpc(State(host): State<Host>, headers: HeaderMap, Json(request): Json<Value>) -> Response {
    if headers.get("cookie").and_then(|value| value.to_str().ok()) != Some("dsh=fixture") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    host.requests.lock().unwrap().push(request.clone());
    let value = match request["method"].as_str() {
        Some("permissionPresets/catalog") => {
            json!({"options":[{"value":"read-only","name":"Read only"},{"value":"workspace-write","name":"Workspace write"},{"value":"custom-policy","name":"Custom policy"}]})
        }
        Some("session/list") => json!({"items":*host.sessions.lock().unwrap()}),
        Some("session/modelCatalog") => {
            json!({"default":{"provider":"local","model":"test"},"groups":[{"id":"local","name":"Local","models":[{"id":"test","name":"Test","reasoning":{"defaultEffort":"low","efforts":[{"id":"low","name":"Low"},{"id":"high","name":"High"}]}}]}]})
        }
        Some("session/page") => {
            let before = request["payload"]["args"]["request"]["beforeSeq"]
                .as_u64()
                .unwrap();
            page(&host, before)
        }
        Some("session/create") => json!({"sessionId":"session"}),
        Some("session/selectModel") => {
            let mut model = request["payload"]["args"]["request"].clone();
            model.as_object_mut().unwrap().remove("sessionId");
            json!({"selected":model})
        }
        Some("commands/execute") => json!({"result":{"kind":"success"}}),
        Some("session/prompt" | "session/cancel") => json!({"accepted":true}),
        Some("$events/result") => json!(null),
        Some("session/attachment") => {
            json!({"attachment":{"attachmentId":"image","mediaType":"image/png"},"data":"aGVsbG8="})
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(json!({"type":"server-response","rpcId":request["rpcId"],"result":{"ok":true,"value":value}})).into_response()
}

pub(super) async fn next(session: &mut dyn AgentSession) -> AgentTurnEvent {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                return event;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native event should arrive")
}

fn page(host: &Host, before: u64) -> Value {
    let records = host.records.lock().unwrap();
    let end = records
        .iter()
        .position(|r| r["event"]["seq"].as_u64().unwrap() >= before)
        .unwrap_or(records.len());
    let start = end.saturating_sub(2);
    json!({"records":records[start..end],"hasMore":start>0})
}
fn snapshot(host: &Host) -> Value {
    let cut = host
        .records
        .lock()
        .unwrap()
        .last()
        .map_or(0, |r| r["event"]["seq"].as_u64().unwrap());
    let page = page(host, cut + 1);
    let mut snapshot = json!({"type":"snapshot","header":{"id":"session","cwd":host.cwd,"createdAt":1_700_000_000_000_i64},"cursor":cut,"records":page["records"],"hasMore":page["hasMore"],"projections":{"values":{"permissions":{"options":[{"value":"read-only","name":"Read only"},{"value":"workspace-write","name":"Workspace write"},{"value":"danger-full-access","name":"Full access"}],"currentValue":"workspace-write"},"modelSelection":{"next":null}}}});
    snapshot.as_object_mut().unwrap().extend(
        host.snapshot_fields
            .lock()
            .unwrap()
            .as_object()
            .unwrap()
            .clone(),
    );
    snapshot
}
