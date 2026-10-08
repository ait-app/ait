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

#[expect(
    clippy::struct_excessive_bools,
    reason = "Each flag independently selects a fixture behavior"
)]
pub(in crate::local::opencode) struct StateData {
    pub(in crate::local::opencode) session_pages: Vec<Value>,
    pub(in crate::local::opencode) session_queries: Vec<Vec<(String, String)>>,
    pub(in crate::local::opencode) pending_plugin_polls: usize,
    pub(in crate::local::opencode) empty_model_catalogs: usize,
    pub(in crate::local::opencode) idle_completion: bool,
    pub(in crate::local::opencode) aborted_completion: bool,
    version: Version,
    cwd: PathBuf,
    pub(in crate::local::opencode) permission: Value,
    pub(in crate::local::opencode) agent: String,
    pub(in crate::local::opencode) permission_updates: usize,
    pub(in crate::local::opencode) model: Value,
    pub(in crate::local::opencode) history: Vec<Value>,
    pub(in crate::local::opencode) submissions: usize,
    pub(in crate::local::opencode) reject_ack: bool,
    pub(in crate::local::opencode) busy: bool,
    pub(in crate::local::opencode) cursor_cycle: bool,
    pub(in crate::local::opencode) early_failure: bool,
    pub(in crate::local::opencode) pending_permissions: Vec<Value>,
    pub(in crate::local::opencode) replies: Vec<Value>,
    pub(in crate::local::opencode) interrupts: usize,
    pub(in crate::local::opencode) reject_interrupt: bool,
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
            session_pages: Vec::new(),
            session_queries: Vec::new(),
            pending_plugin_polls: 0,
            empty_model_catalogs: 0,
            idle_completion: false,
            aborted_completion: false,
            version,
            cwd: cwd.clone(),
            permission: Value::Null,
            agent: "build".into(),
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
            interrupts: 0,
            reject_interrupt: false,
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
    if let Some(response) = session_page(&state, &request) {
        return response;
    }
    if matches!(path.as_str(), "/api/model" | "/api/plugin")
        && !valid_location_query(&request, &state.lock().unwrap().cwd)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
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
        let body = if state.idle_completion || state.aborted_completion {
            format!(
                "data: {}\n\n",
                json!({"type":"log.synced","aggregateID":"ses_one","seq":state.submissions})
            )
        } else if state.history.is_empty() {
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
    fixture_response(fixture_json(&mut state, method.as_str(), &path, body), v2)
}

fn fixture_response(result: Result<Value, StatusCode>, v2: bool) -> Response {
    let response = match result {
        Ok(response) => response,
        Err(status) => return status.into_response(),
    };
    if v2 && !response.is_null() && response.get("data").is_none() {
        Json(json!({"data":response})).into_response()
    } else {
        Json(response).into_response()
    }
}

fn session_page(state: &Arc<Mutex<StateData>>, request: &Request) -> Option<Response> {
    let path = request.uri().path();
    let method = request.method();
    if method == "GET" && matches!(path, "/api/session" | "/experimental/session") {
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
        let mut data = state.lock().unwrap();
        let page = data.session_queries.len();
        data.session_queries
            .push(url.query_pairs().into_owned().collect());
        if let Some(response) = data.session_pages.get(page) {
            return Some(Json(response.clone()).into_response());
        }
    }
    None
}

fn valid_location_query(request: &Request, cwd: &std::path::Path) -> bool {
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
    let query = url.query_pairs().collect::<Vec<_>>();
    query.len() == 1 && query[0].0 == "location[directory]" && query[0].1 == cwd.to_string_lossy()
}

fn patch_permissions(state: &mut StateData, v2: bool, body: &Value) -> Value {
    state.permission_updates += 1;
    if v2 {
        state.permission = body["permissions"].clone();
    } else {
        // OpenCode 1.18.33 SessionHttpApi.update uses Permission.merge (flat).
        let mut rules = state.permission.as_array().cloned().unwrap_or_default();
        rules.extend(body["permission"].as_array().unwrap().iter().cloned());
        state.permission = json!(rules);
    }
    if v2 { Value::Null } else { session_info(state) }
}

fn fixture_json(
    state: &mut StateData,
    method: &str,
    path: &str,
    body: Value,
) -> Result<Value, StatusCode> {
    let v2 = state.version == Version::V2;
    let response = match (method, path) {
        ("GET", "/global/health" | "/api/info") => json!({"healthy":true}),
        ("GET", "/provider") => {
            json!({"connected":["local"],"all":[{"id":"local","models":{"test-model":{"name":"Test model","variants":{"high":{}}}}}]})
        }
        ("GET", "/api/plugin") => {
            if state.pending_plugin_polls > 0 {
                state.pending_plugin_polls -= 1;
                json!({"data":[]})
            } else {
                json!({"data":[{"id":"config","state":{"status":"active"}}]})
            }
        }
        ("GET", "/api/model") => model_catalog(state),
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
        ("GET", "/experimental/session" | "/api/session") => {
            json!([session_info(state)])
        }
        ("POST", "/session" | "/api/session") => {
            let key = if v2 { "permissions" } else { "permission" };
            if let Some(permission) = body.get(key) {
                state.permission = permission.clone();
            }
            state.agent = body["agent"].as_str().unwrap_or("build").into();
            state.model = body["model"].clone();
            session_info(state)
        }
        ("PATCH", "/session/ses_one" | "/api/session/ses_one") => {
            patch_permissions(state, v2, &body)
        }

        ("POST", "/api/session/ses_one/agent") => {
            state.agent = body["agent"].as_str().unwrap().into();
            Value::Null
        }
        ("POST", "/api/session/ses_one/model") => {
            state.model = body["model"].clone();
            Value::Null
        }
        ("GET", "/session/ses_one" | "/api/session/ses_one") => session_info(state),
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
            state.replies.push(body);
            state.pending_permissions.clear();
            state.busy = state.stream_after_permission;
            Value::Null
        }
        ("POST", "/session/ses_one/abort" | "/api/session/ses_one/interrupt") => {
            return interrupt_session(state);
        }
        ("POST", "/session/ses_one/prompt_async" | "/api/session/ses_one/prompt") => {
            return record_prompt(state, &body);
        }
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(response)
}

fn interrupt_session(state: &mut StateData) -> Result<Value, StatusCode> {
    state.interrupts += 1;
    if state.reject_interrupt {
        return Err(StatusCode::BAD_GATEWAY);
    }
    state.busy = false;
    state.pending_permissions.clear();
    for message in &mut state.history {
        let info = if state.version == Version::V2 {
            message
        } else {
            &mut message["info"]
        };
        if (info["type"] == "assistant" || info["role"] == "assistant")
            && info.pointer("/time/completed").is_none()
        {
            info["time"]["completed"] = json!(13);
            if state.version == Version::V2 {
                info["finish"] = json!("error");
                info["error"] = json!({"type":"aborted","message":"Step interrupted"});
            }
        }
    }
    Ok(Value::Null)
}

fn record_prompt(state: &mut StateData, body: &Value) -> Result<Value, StatusCode> {
    state.submissions += 1;
    if let Some(agent) = body["agent"].as_str() {
        state.agent = agent.into();
    }
    state.busy |= !state.pending_permissions.is_empty();
    let number = state.submissions;
    if state.version == Version::V2 {
        state.history.extend([json!({"id":format!("user{number}"),"type":"user","text":body["text"],"metadata":body["metadata"],"time":{"created":10}}),
            json!({"id":format!("answer{number}"),"type":"assistant","time":{"created":11,"completed":12},"content":[{"type":"text","text":"answer"}]})]);
    } else {
        state.history.extend([json!({"info":{"id":body["messageID"],"sessionID":"ses_one","role":"user","time":{"created":10}},"parts":[{"id":"pu","sessionID":"ses_one","messageID":body["messageID"],"type":"text","text":body["parts"][0]["text"]}]}),
            json!({"info":{"id":format!("msg_0123456789abABCDEFGHIJKLM{number}"),"sessionID":"ses_one","role":"assistant","time":{"created":11,"completed":12}},"parts":[{"id":format!("prt_0123456789abABCDEFGHIJKLM{number}"),"sessionID":"ses_one","messageID":format!("msg_0123456789abABCDEFGHIJKLM{number}"),"type":"text","text":"answer"}]})]);
    }
    if state.early_failure {
        state.history.pop();
    }
    if state.idle_completion {
        state
            .history
            .push(json!({"id":format!("idle{number}"),"type":"idle",
            "time":{"created":13},"outcome":if state.early_failure {"failed"} else {"succeeded"}}));
    }
    if state.aborted_completion {
        let last = state.history.last_mut().unwrap();
        last["finish"] = json!("error");
        last["error"] = json!({"type":"aborted","message":"Step interrupted"});
    }
    if state.reject_ack {
        return Err(StatusCode::BAD_GATEWAY);
    }
    Ok(Value::Null)
}

fn session_info(state: &StateData) -> Value {
    if state.version == Version::V2 {
        json!({"id":"ses_one","title":"Existing session","time":{"created":1,"updated":13},"location":{"directory":state.cwd},"model":state.model,
        "agent":state.agent,"permissions":state.permission,"outcome":if state.history.is_empty() || state.aborted_completion {None} else {Some(if state.early_failure {"failed"} else {"succeeded"})}})
    } else {
        json!({"id":"ses_one","title":"Existing session","time":{"created":1,"updated":13},"directory":state.cwd,"permission":state.permission})
    }
}

fn model_catalog(state: &mut StateData) -> Value {
    if state.pending_plugin_polls > 0 {
        return json!({"data":[{"providerID":"bootstrap","id":"partial","enabled":true}]});
    }
    if state.empty_model_catalogs > 0 {
        state.empty_model_catalogs -= 1;
        return json!({"data":[]});
    }
    json!({"data":[{"providerID":"local","id":"test-model","name":"Test model","enabled":true,"variants":[{"id":"high"}]}]})
}
