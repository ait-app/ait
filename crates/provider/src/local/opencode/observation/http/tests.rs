//! Read-only, authenticated loopback acceptance for the native metadata boundary.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use futures_util::stream;
use tokio::sync::broadcast;

use super::super::tests::{catalog, message};
use super::super::*;

#[derive(Debug)]
struct Native {
    authorization: String,
    events: broadcast::Sender<Value>,
    requests: Mutex<Vec<String>>,
    cycle: AtomicBool,
}

fn fixture() -> (Source, Arc<Native>, AbortOnDropHandle<()>) {
    use base64::Engine;
    let mut source = Source::prepare("1.18.4", Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let mut command = Command::new("unused");
    source.configure(&mut command).unwrap();
    let arguments: Vec<_> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect();
    let port: u16 = arguments[arguments.iter().position(|arg| *arg == "--port").unwrap() + 1]
        .parse()
        .unwrap();
    let password = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == "OPENCODE_SERVER_PASSWORD")
        .unwrap()
        .1
        .unwrap()
        .to_str()
        .unwrap();
    let authorization = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
    );
    assert!(!format!("{source:?}").contains(password));
    // Transfer the reserved socket without a close/rebind gap under parallel process startup.
    let listener = source.prepared.take().unwrap().listener;
    assert_eq!(listener.local_addr().unwrap().port(), port);
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let native = Arc::new(Native {
        authorization,
        events: broadcast::channel(32).0,
        requests: Mutex::new(Vec::new()),
        cycle: AtomicBool::new(false),
    });
    let router = Router::new().fallback(serve).with_state(native.clone());
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    (source, native, task)
}

async fn serve(
    State(native): State<Arc<Native>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    assert_eq!(method, Method::GET);
    assert_eq!(
        query.get("directory").map(String::as_str),
        Some("/workspace")
    );
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(&native.authorization)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    native.requests.lock().unwrap().push(uri.path().to_owned());
    match uri.path() {
        "/event" => {
            let events = stream::unfold(native.events.subscribe(), |mut receiver| async move {
                let event = receiver.recv().await.ok()?;
                Some((
                    Ok::<_, std::io::Error>(format!(
                        ": heartbeat\r\nevent: update\r\ndata: {event}\r\n\r\n"
                    )),
                    receiver,
                ))
            });
            (
                [("content-type", "text/event-stream")],
                Body::from_stream(events),
            )
                .into_response()
        }
        "/config/providers" => axum::Json(catalog()).into_response(),
        "/session/parent" => axum::Json(json!({"id":"parent","cost":0.02})).into_response(),
        "/session/parent/message" => {
            let mut response = if query.contains_key("before") {
                assert_eq!(query["before"], "opaque=/+?&");
                axum::Json(json!([
                    {"info":{"id":"compact","sessionID":"parent","role":"user"},
                        "parts":[{"type":"compaction","auto":true}]},
                    {"info":{"id":"summary","sessionID":"parent","role":"assistant",
                        "summary":true,"parentID":"compact","finish":"stop","time":{"created":1}},
                        "parts":[{"type":"text","text":"private transcript"}]}
                ]))
                .into_response()
            } else {
                let info = message("real", 2, 251, 629_632)["properties"]["info"].clone();
                let empty = message("placeholder", 3, 0, 0)["properties"]["info"].clone();
                axum::Json(json!([{"info":info,"parts":[]},{"info":empty,"parts":[]}]))
                    .into_response()
            };
            if !query.contains_key("before") || native.cycle.load(Ordering::SeqCst) {
                response
                    .headers_mut()
                    .insert("x-next-cursor", "opaque=/+?&".parse().unwrap());
            }
            response
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[tokio::test]
async fn owned_native_observation_restores_cached_usage_and_compaction_then_streams_new_usage() {
    let (source, native, _server) = fixture();
    let mut observation = source.bind("parent", "/workspace").await.unwrap();
    assert_eq!(
        observation.usage().unwrap().context_window_used_tokens,
        Some(629_883)
    );
    assert_eq!(observation.usage().unwrap().total_cost_usd, Some(0.02));
    assert_eq!(observation.initial.len(), 1);
    let mut entries = vec![NativeItem {
        key: "summary".into(),
        turn_id: Some("accepted".into()),
        timestamp: "2026-10-19T00:00:00Z".into(),
        item: json!({"type":"assistant_message","messageId":"summary"}),
    }];
    observation.history(&mut entries, &BTreeMap::new());
    assert_eq!(entries[0].item["status"], "completed");
    assert!(!format!("{observation:?}").contains("private transcript"));
    native.events.send(message("new", 4, 12_000, 0)).unwrap();
    let updates = tokio::time::timeout(Duration::from_secs(5), observation.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updates[0]["used"], 12_000);
    assert!(native.requests.lock().unwrap().iter().all(|path| matches!(
        path.as_str(),
        "/event" | "/config/providers" | "/session/parent" | "/session/parent/message"
    )));
}

#[tokio::test]
async fn repeated_native_history_cursors_fail_boundedly_without_following_server_urls() {
    let (source, native, _server) = fixture();
    native.cycle.store(true, Ordering::SeqCst);
    assert!(matches!(
        source.bind("parent", "/workspace").await,
        Err(AgentSessionError::Failed)
    ));
    assert_eq!(
        native
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/session/parent/message")
            .count(),
        2
    );
}
