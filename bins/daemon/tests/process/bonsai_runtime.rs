use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};

use super::transport::{connect, request};
use super::{ready, start_logged, start_with_environment, terminate};

const RUNTIME_ID: &str = "rt_0123456789abcdef0123456789abcdef";
const RUNTIME_TOKEN: &str = "fedcba9876543210fedcba9876543210";
const RUN: &str = "r_00112233445566778899aabbccddeeff";
const WAIT: Duration = Duration::from_secs(20);

type Hub = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// Write the Claude fixture as the server's Claude binary.
fn claude(root: &std::path::Path) {
    let peer = root.join("claude_code.py");
    std::fs::write(
        &peer,
        include_str!("../../../../crates/provider/tests/fixtures/claude_code.py"),
    )
    .expect("write Claude peer");
    let program = root.join("claude");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nexec /usr/bin/env python3 '{}' \"$@\"\n",
            peer.display()
        ),
    )
    .expect("write Claude wrapper");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
        .expect("make wrapper executable");
}

/// The head of a frame (its first line) as JSON, and the body after it, if any.
fn frame(text: &str) -> (Value, Option<Value>) {
    let (head, body) = text
        .split_once('\n')
        .map_or((text, None), |(head, body)| (head, Some(body)));
    (
        serde_json::from_str(head).expect("a JSON head"),
        body.map(|body| serde_json::from_str(body).expect("a JSON body")),
    )
}

/// Next text frame from the adapter, answering nothing but skipping heartbeats.
async fn next(hub: &mut Hub) -> (Value, Option<Value>) {
    loop {
        let message = tokio::time::timeout(WAIT, hub.next())
            .await
            .expect("the adapter sends a frame")
            .expect("the connection stays open")
            .expect("a readable frame");
        if let Message::Text(text) = message {
            if text.as_str() == "ping" {
                hub.send(Message::Text("pong".into())).await.expect("pong");
                continue;
            }
            return frame(text.as_str());
        }
    }
}

/// What the runtime's WebSocket upgrade request carried.
#[derive(Clone, Default)]
struct Handshake(Arc<Mutex<(Option<String>, Option<String>)>>);

impl Callback for Handshake {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let authorization = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        *self.0.lock().expect("lock") = (authorization, Some(request.uri().path().to_owned()));
        Ok(response)
    }
}

/// Register the fixture's project, then restart the server configured for a Bonsai at `origin`.
async fn configured_server(
    fixture: &super::native::NativeFixture,
    origin: &str,
) -> (super::Process, PathBuf) {
    let root = fixture.root.path();
    claude(root);
    let state = root.join("state");
    let log = root.join("server.log");
    let mut process = start_with_environment(&state, &log, Some(&fixture.path), &[]);
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &["workspace.open.request"]).await;
    request(
        &mut client,
        "workspace.open.request",
        json!({"cwd": fixture.cwd}),
    )
    .await;
    terminate(&mut process).await;
    let log = root.join("daemon-bonsai.log");
    // At trace the WebSocket library would print the raw handshake request, token included.
    let mut process = start_logged(
        &state,
        &log,
        Some(&fixture.path),
        &[
            ("BONSAI_RUNTIME_URL", origin),
            ("BONSAI_RUNTIME_ID", RUNTIME_ID),
            ("BONSAI_RUNTIME_TOKEN", RUNTIME_TOKEN),
        ],
        "trace",
    );
    ready(&mut process, &log).await;
    (process, log)
}

/// Statuses and events until the first turn completes.
async fn first_turn(hub: &mut Hub) -> (Vec<Value>, Vec<Value>) {
    let mut statuses = Vec::new();
    let mut events = Vec::new();
    while !events
        .iter()
        .any(|event: &Value| event["t"] == "turn" && event["state"] == "completed")
    {
        let (head, body) = next(hub).await;
        match head["type"].as_str() {
            Some("run.status") => statuses.push(head),
            Some("session.events") => {
                events.extend(
                    body.and_then(|body| body.as_array().cloned())
                        .unwrap_or_default(),
                );
            }
            other => panic!("unexpected frame {other:?}: {head}"),
        }
    }
    (statuses, events)
}

fn dispatch(project: &str) -> String {
    json!({
        "type": "run.dispatch", "run_id": RUN, "space_id": "space", "session": "bonsai.session/1",
        "task": {"path": "Projects/Note.md", "line": 3, "text": "- [ ] say hello", "heading": null, "context": ""},
        "project": {"id": project}, "provider": "claude", "model": null, "instruction": "",
        "settings": {}, "wrapup": "wrap up", "bonsai": {"mcp_url": "https://bonsai.example/mcp"},
        "requested_by": {"id": "github:2", "login": "member", "owner": false}
    })
    .to_string()
}

#[tokio::test]
async fn a_dispatch_runs_through_the_real_server_and_streams_its_session() {
    // Arrange
    let fixture = super::native::NativeFixture::new();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the hub");
    let origin = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("address").port()
    );
    let (mut process, log) = configured_server(&fixture, &origin).await;

    // Act: accept the runtime's connection and read its hello.
    let (stream, _) = tokio::time::timeout(WAIT, listener.accept())
        .await
        .expect("the runtime dials out")
        .expect("a connection");
    let handshake = Handshake::default();
    let mut hub = tokio_tungstenite::accept_hdr_async(stream, handshake.clone())
        .await
        .expect("a WebSocket handshake");
    let (hello, _) = next(&mut hub).await;

    // Assert
    let (authorization, path) = handshake.0.lock().expect("lock").clone();
    assert_eq!(path.as_deref(), Some("/runtime"));
    assert_eq!(authorization, Some(format!("Bearer {RUNTIME_TOKEN}")));
    assert_eq!(hello["type"], "runtime.hello");
    assert_eq!(hello["runtime_id"], RUNTIME_ID);
    assert_eq!(hello["session"], json!(["bonsai.session/1"]));
    let project = hello["projects"][0]["id"]
        .as_str()
        .expect("the project is announced")
        .to_owned();
    assert!(
        !hello
            .to_string()
            .contains(fixture.cwd.to_str().expect("a UTF-8 path"))
    );
    assert!(!hello.to_string().contains(RUNTIME_TOKEN));

    // Act: welcome, dispatch to the fixture Claude, then cancel the idle run.
    let welcome = json!({"type": "runtime.welcome", "owner": {"id": "github:1", "login": "owner"}});
    hub.send(Message::Text(welcome.to_string().into()))
        .await
        .expect("welcome");
    hub.send(Message::Text(dispatch(&project).into()))
        .await
        .expect("dispatch");
    let (mut statuses, events) = first_turn(&mut hub).await;
    let cancel = json!({"type": "run.cancel", "run_id": RUN});
    hub.send(Message::Text(cancel.to_string().into()))
        .await
        .expect("cancel");
    while statuses
        .last()
        .is_none_or(|status| status["status"] == "claimed" || status["status"] == "running")
    {
        let (head, _) = next(&mut hub).await;
        if head["type"] == "run.status" {
            statuses.push(head);
        }
    }
    terminate(&mut process).await;

    // Assert: claimed, running and a final state, each with the execution; a numbered log.
    let states: Vec<&str> = statuses
        .iter()
        .filter_map(|status| status["status"].as_str())
        .collect();
    assert_eq!(states[..2], ["claimed", "running"], "{statuses:?}");
    assert_eq!(
        states.last(),
        Some(&"completed"),
        "an idle session completes: {statuses:?}"
    );
    for status in &statuses {
        assert_eq!(status["execution"]["provider"], "claude", "{status}");
        assert_eq!(
            status["execution"]["bonsai_write"], false,
            "a remote Bonsai is never written"
        );
    }
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["t"].as_str())
        .collect();
    assert_eq!(kinds[..2], ["input", "turn"], "{events:?}");
    assert_eq!(events[0]["origin"], "dispatch");
    assert!(kinds.contains(&"text"), "{events:?}");
    for (seq, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], seq, "{events:?}");
    }
    let server_log = std::fs::read_to_string(&log).expect("the server log");
    assert!(server_log.contains(" TRACE "), "the server logged at trace");
    assert!(!server_log.contains(RUNTIME_TOKEN), "{server_log}");
    assert!(!server_log.to_ascii_lowercase().contains("authorization"));
}
