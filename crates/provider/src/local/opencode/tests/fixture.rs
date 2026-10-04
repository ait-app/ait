#![allow(clippy::too_many_lines, clippy::struct_excessive_bools)]
use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::any,
};
use serde_json::{Value, json};
use tokio_stream::StreamExt;
use tokio_util::task::AbortOnDropHandle;

use super::super::http::Version;

pub(in crate::local::opencode) struct StateData {
    version: Version,
    cwd: PathBuf,
    pub(in crate::local::opencode) permission: Value,
    pub(in crate::local::opencode) permission_updates: usize,
    model: Value,
    pub(in crate::local::opencode) history: Vec<Value>,
    pub(in crate::local::opencode) submissions: usize,
    pub(in crate::local::opencode) reject_ack: bool,
    pub(in crate::local::opencode) busy: bool,
    pub(in crate::local::opencode) cursor_cycle: bool,
    pub(in crate::local::opencode) early_failure: bool,
    pub(in crate::local::opencode) pending_permissions: Vec<Value>,
    pub(in crate::local::opencode) replies: Vec<Value>,
    pub(in crate::local::opencode) stream_text: bool,
    pub(in crate::local::opencode) stream_after_permission: bool,
    pub(in crate::local::opencode) unfinished_while_busy: bool,
    pub(in crate::local::opencode) omit_input_history: bool,
    pub(in crate::local::opencode) stream_events: Vec<Value>,
}

pub(in crate::local::opencode) struct Fixture {
    pub(in crate::local::opencode) binary: PathBuf,
    pub(in crate::local::opencode) cwd: PathBuf,
    pub(in crate::local::opencode) state: Arc<Mutex<StateData>>,
    _directory: tempfile::TempDir,
    _server: AbortOnDropHandle<()>,
}

impl Fixture {
    pub(in crate::local::opencode) async fn start(version: Version) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().canonicalize().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let binary = directory.path().join("opencode-fixture");
        let (release, password) = match version {
            Version::V1 => ("1.18.33", "OPENCODE_SERVER_PASSWORD"),
            Version::V2 => ("2.0.10", "OPENCODE_PASSWORD"),
        };
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo '{release}'; exit 0; fi\n[ \"$1 $2 $3 $4 $5\" = 'serve --hostname 127.0.0.1 --port 0' ] || exit 12\n[ -n \"${password}\" ] || exit 13\necho 'opencode server listening on http://{address}'\nexec sleep 600\n"
        );
        std::fs::write(&binary, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let state = Arc::new(Mutex::new(StateData {
            version,
            cwd: cwd.clone(),
            permission: Value::Null,
            permission_updates: 0,
            model: Value::Null,
            history: Vec::new(),
            submissions: 0,
            reject_ack: false,
            busy: false,
            cursor_cycle: false,
            early_failure: false,
            pending_permissions: Vec::new(),
            replies: Vec::new(),
            stream_text: false,
            stream_after_permission: false,
            unfinished_while_busy: false,
            omit_input_history: false,
            stream_events: Vec::new(),
        }));
        let router = Router::new()
            .fallback(any(handle))
            .with_state(state.clone());
        let server = AbortOnDropHandle::new(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }));
        Self {
            binary,
            cwd,
            state,
            _directory: directory,
            _server: server,
        }
    }
}

async fn handle(State(state): State<Arc<Mutex<StateData>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    if !request
        .headers()
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .is_some_and(|value| value.starts_with("Basic "))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if path.ends_with("/event") {
        let event = Event::default()
            .json_data(json!({"type":"server.connected"}))
            .unwrap();
        let mut events = vec![Ok::<_, Infallible>(event)];
        let data = state.lock().unwrap();
        events.extend(
            data.stream_events
                .iter()
                .map(|value| Ok(Event::default().json_data(value).unwrap())),
        );
        if data.stream_text {
            if data.version == Version::V1 {
                events.push(Ok(Event::default().json_data(json!({"type":"message.updated","properties":{"info":{"id":"msg_0123456789abABCDEFGHIJKLM1","sessionID":"ses_one","role":"assistant"}}})).unwrap()));
            }
            let value = match data.version {
                Version::V1 => {
                    json!({"type":"message.part.updated","properties":{"part":{"id":"prt_0123456789abABCDEFGHIJKLM1","messageID":"msg_0123456789abABCDEFGHIJKLM1","sessionID":"ses_one","type":"text","text":"ans"}}})
                }
                Version::V2 => {
                    json!({"type":"session.text.delta","data":{"sessionID":"ses_one","assistantMessageID":"answer1","delta":"ans"}})
                }
            };
            events.push(Ok(Event::default().json_data(value).unwrap()));
        }
        let wait = data.stream_after_permission;
        drop(data);
        let stream =
            tokio_stream::iter(events.into_iter().enumerate()).then(move |(index, event)| {
                let state = state.clone();
                async move {
                    if wait && index > 0 {
                        while state.lock().unwrap().replies.is_empty() {
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        }
                    }
                    event
                }
            });
        return Sse::new(stream.chain(tokio_stream::pending())).into_response();
    }
    let body = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    if ["/variant", "/system", "/model/variant"]
        .iter()
        .any(|field| body.pointer(field).is_some_and(Value::is_null))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut state = state.lock().unwrap();
    let v2 = state.version == Version::V2;
    if path == "/api/experimental/session/ses_one/log" {
        let body = if state.history.is_empty() {
            String::new()
        } else {
            format!(
                "data: {}\n\n",
                json!({"type":if state.early_failure {"session.execution.failed"} else {"session.execution.succeeded"}, "created":13,
                "durable":{"aggregateID":"ses_one","seq":state.submissions}, "data":{"sessionID":"ses_one"}})
            )
        };
        return ([("content-type", "text/event-stream")], body).into_response();
    }
    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/global/health" | "/api/info") => json!({"healthy":true}),
        ("GET", "/provider") => {
            json!({"connected":["local"],"all":[{"id":"local","models":{"test-model":{"name":"Test model","variants":{"high":{}}}}}]})
        }
        ("GET", "/api/model") => {
            json!({"data":[{"providerID":"local","id":"test-model","name":"Test model","enabled":true,"variants":[{"id":"high"}]}]})
        }
        ("GET", "/session/status" | "/api/session/active") => {
            if state.busy {
                if v2 {
                    json!({"data":{"ses_one":{}}})
                } else {
                    json!({"ses_one":{"type":"retry"}})
                }
            } else if v2 {
                json!({"data":{}})
            } else {
                json!({})
            }
        }
        ("POST", "/session" | "/api/session") => {
            state.permission = body[if v2 { "permissions" } else { "permission" }].clone();
            state.model = body["model"].clone();
            session_info(&state)
        }
        ("PATCH", "/session/ses_one" | "/api/session/ses_one") => {
            state.permission_updates += 1;
            if v2 {
                state.permission = body["permissions"].clone();
            } else {
                // OpenCode 1.18.33 SessionHttpApi.update uses Permission.merge (flat).
                state
                    .permission
                    .as_array_mut()
                    .unwrap()
                    .extend(body["permission"].as_array().unwrap().iter().cloned());
            }
            if v2 {
                Value::Null
            } else {
                session_info(&state)
            }
        }
        ("POST", "/api/session/ses_one/model") => {
            state.model = body["model"].clone();
            Value::Null
        }
        ("GET", "/session/ses_one" | "/api/session/ses_one") => session_info(&state),
        ("GET", "/session/ses_one/message" | "/api/session/ses_one/message") => {
            let mut history = state.history.clone();
            if state.omit_input_history {
                history.retain(|message| {
                    if v2 {
                        message["type"] != "user"
                    } else {
                        message["info"]["role"] != "user"
                    }
                });
            }
            if state.busy && state.unfinished_while_busy {
                for message in &mut history {
                    let info = if v2 { message } else { &mut message["info"] };
                    info["time"].as_object_mut().unwrap().remove("completed");
                }
            }
            if v2 {
                json!({"data":history,"cursor":{"next":if state.cursor_cycle {Some("same")} else {None}}})
            } else {
                json!(history)
            }
        }
        ("GET", "/permission" | "/api/session/ses_one/permission") => {
            if v2 {
                json!({"data":state.pending_permissions})
            } else {
                json!(state.pending_permissions)
            }
        }
        ("POST", "/permission/perm1/reply" | "/api/session/ses_one/permission/perm1/reply") => {
            state.replies.push(body.clone());
            state.pending_permissions.clear();
            state.busy = state.stream_after_permission;
            Value::Null
        }
        ("POST", "/session/ses_one/abort" | "/api/session/ses_one/interrupt") => {
            state.busy = false;
            state.pending_permissions.clear();
            Value::Null
        }
        ("POST", "/session/ses_one/prompt_async" | "/api/session/ses_one/prompt") => {
            state.submissions += 1;
            state.busy |= !state.pending_permissions.is_empty();
            let number = state.submissions;
            if v2 {
                state.history.extend([json!({"id":format!("user{number}"),"type":"user","text":body["text"],"metadata":body["metadata"],"time":{"created":10}}),
                    json!({"id":format!("answer{number}"),"type":"assistant","time":{"created":11,"completed":12},"content":[{"type":"text","text":"answer"}]})]);
            } else {
                state.history.extend([json!({"info":{"id":body["messageID"],"sessionID":"ses_one","role":"user","time":{"created":10}},"parts":[{"id":"pu","sessionID":"ses_one","messageID":body["messageID"],"type":"text","text":body["parts"][0]["text"]}]}),
                    json!({"info":{"id":format!("msg_0123456789abABCDEFGHIJKLM{number}"),"sessionID":"ses_one","role":"assistant","time":{"created":11,"completed":12}},"parts":[{"id":format!("prt_0123456789abABCDEFGHIJKLM{number}"),"sessionID":"ses_one","messageID":format!("msg_0123456789abABCDEFGHIJKLM{number}"),"type":"text","text":"answer"}]})]);
            }
            if state.early_failure {
                state.history.pop();
            }
            if state.reject_ack {
                return StatusCode::BAD_GATEWAY.into_response();
            }
            Value::Null
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    if v2 && !response.is_null() && response.get("data").is_none() {
        Json(json!({"data":response})).into_response()
    } else {
        Json(response).into_response()
    }
}

fn session_info(state: &StateData) -> Value {
    if state.version == Version::V2 {
        json!({"id":"ses_one","location":{"directory":state.cwd},"model":state.model,
        "permissions":state.permission,"outcome":if state.history.is_empty() {None} else {Some(if state.early_failure {"failed"} else {"succeeded"})}})
    } else {
        json!({"id":"ses_one","directory":state.cwd,"permission":state.permission})
    }
}
