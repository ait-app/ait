use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec, AgentTurnEvent};
use serde_json::{Value, json};
use tokio::process::Command;

use super::{Opened, cancel, close, open, persistence, respond, resumable, withdraw};
use crate::acp_permissions::{Pending, capture};
use crate::acp_transport::Transport;

const PROVIDER: &str = "minimax";

/// Fake agent that echoes every received frame as an `echo` notification, then answers each
/// request whose method appears in `replies` with the configured result.
const AGENT: &str = r#"import json, sys
replies = json.loads(sys.argv[1])
for line in sys.stdin:
    message = json.loads(line)
    print(json.dumps({"jsonrpc": "2.0", "method": "echo", "params": message}), flush=True)
    if "id" in message and message.get("method") in replies:
        reply = {"jsonrpc": "2.0", "id": message["id"], "result": replies[message["method"]]}
        print(json.dumps(reply), flush=True)
"#;

fn agent(replies: &Value) -> Transport {
    let mut command = Command::new("python3");
    command.args(["-c", AGENT, &replies.to_string()]);
    Transport::spawn(command, ".", Duration::from_secs(3), "test").unwrap()
}

/// Frames the agent received, in order, as echoed back so far.
fn received(transport: &mut Transport) -> Vec<Value> {
    std::iter::from_fn(|| transport.poll().ok().flatten())
        .filter(|message| message["method"] == "echo")
        .map(|message| message["params"].clone())
        .collect()
}

async fn receive_echo(transport: &mut Transport) -> Value {
    loop {
        let message = transport.receive().await.unwrap();
        if message["method"] == "echo" {
            return message["params"].clone();
        }
    }
}

fn spec() -> AgentSessionSpec {
    AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: "/work".to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    }
}

fn retained() -> BTreeMap<String, Pending> {
    let message = json!({"id":7,"params":{"toolCall":{"toolCallId":"tool","title":"shell"},
        "options":[{"optionId":"yes","name":"Allow","kind":"allow_once"},
            {"optionId":"no","name":"Deny","kind":"reject_once"}]}});
    BTreeMap::from([("request".to_owned(), capture(&message, PROVIDER).unwrap())])
}

#[test]
fn recorded_handles_resume_only_the_same_provider_and_directory() {
    let handle = persistence(PROVIDER, "native", "/work");
    assert_eq!(
        handle,
        AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: "native".to_owned(),
            native_handle: None,
            metadata: Some(BTreeMap::from([("cwd".to_owned(), json!("/work"))])),
        }
    );
    assert!(resumable(&handle, PROVIDER, "/work"));
    assert!(!resumable(&handle, "opencode", "/work"));
    assert!(!resumable(&handle, PROVIDER, "/elsewhere"));
    for session_id in [String::new(), "a\nb".to_owned(), "x".repeat(1025)] {
        let invalid = persistence(PROVIDER, &session_id, "/work");
        assert!(!resumable(&invalid, PROVIDER, "/work"));
    }
    assert!(resumable(
        &persistence(PROVIDER, &"x".repeat(1024), "/work"),
        PROVIDER,
        "/work"
    ));
    let without_directory = AgentPersistenceHandle {
        metadata: None,
        ..handle
    };
    assert!(!resumable(&without_directory, PROVIDER, "/work"));
}

#[tokio::test]
async fn open_initializes_and_creates_a_session_with_the_client_capabilities() {
    let mut transport = agent(&json!({
        "initialize":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}},
        "session/new":{"sessionId":"native","configOptions":[]}
    }));
    let Opened {
        capabilities,
        result,
        id,
    } = open(
        &mut transport,
        json!({"elicitation":{"form":{}}}),
        &spec(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(id, "native");
    assert_eq!(capabilities, json!({"loadSession":true}));
    assert_eq!(result["configOptions"], json!([]));
    let frames = received(&mut transport);
    assert_eq!(frames[0]["method"], "initialize");
    assert_eq!(frames[0]["params"]["protocolVersion"], 1);
    assert_eq!(
        frames[0]["params"]["clientCapabilities"],
        json!({"elicitation":{"form":{}}})
    );
    assert_eq!(frames[0]["params"]["clientInfo"]["name"], "ait");
    assert_eq!(frames[1]["method"], "session/new");
    assert_eq!(frames[1]["params"], json!({"cwd":"/work","mcpServers":[]}));
    transport.close().await.unwrap();
}

#[tokio::test]
async fn open_resumes_the_handle_and_rejects_a_different_reported_session() {
    let handle = persistence(PROVIDER, "native", "/work");
    let resumable_agent = |reported: Value| {
        agent(&json!({
            "initialize":{"protocolVersion":1,
                "agentCapabilities":{"sessionCapabilities":{"resume":{}}}},
            "session/resume":reported
        }))
    };

    let mut transport = resumable_agent(json!({}));
    let opened = open(&mut transport, json!({}), &spec(), Some(&handle))
        .await
        .unwrap();
    assert_eq!(opened.id, "native");
    let frames = received(&mut transport);
    assert_eq!(frames[1]["method"], "session/resume");
    assert_eq!(
        frames[1]["params"],
        json!({"cwd":"/work","mcpServers":[],"sessionId":"native"})
    );
    transport.close().await.unwrap();

    let mut transport = resumable_agent(json!({"sessionId":"native"}));
    let opened = open(&mut transport, json!({}), &spec(), Some(&handle)).await;
    assert_eq!(opened.unwrap().id, "native");
    transport.close().await.unwrap();

    let mut transport = resumable_agent(json!({"sessionId":"other"}));
    let opened = open(&mut transport, json!({}), &spec(), Some(&handle)).await;
    assert_eq!(opened.unwrap_err(), AgentSessionError::Rejected);
    transport.close().await.unwrap();
}

#[tokio::test]
async fn open_rejects_unsupported_protocols_resumes_and_anonymous_sessions() {
    let handle = persistence(PROVIDER, "native", "/work");
    let mut transport = agent(&json!({"initialize":{"protocolVersion":2}}));
    let opened = open(&mut transport, json!({}), &spec(), None).await;
    assert_eq!(opened.unwrap_err(), AgentSessionError::Failed);
    transport.close().await.unwrap();

    let mut transport = agent(&json!({
        "initialize":{"protocolVersion":1,"agentCapabilities":{}},
        "session/resume":{"sessionId":"native"}
    }));
    let opened = open(&mut transport, json!({}), &spec(), Some(&handle)).await;
    assert_eq!(opened.unwrap_err(), AgentSessionError::Rejected);
    assert!(
        received(&mut transport)
            .iter()
            .all(|message| message["method"] != "session/resume")
    );
    transport.close().await.unwrap();

    let mut transport = agent(&json!({
        "initialize":{"protocolVersion":1,"agentCapabilities":{}},
        "session/new":{}
    }));
    let opened = open(&mut transport, json!({}), &spec(), None).await;
    assert_eq!(opened.unwrap_err(), AgentSessionError::Failed);
    transport.close().await.unwrap();
}

#[tokio::test]
async fn cancel_withdraws_every_callback_before_cancelling_the_prompt() {
    let mut transport = agent(&json!({}));
    let mut pending = retained();
    let mut events = VecDeque::new();
    cancel(&mut transport, "native", &mut pending, &mut events)
        .await
        .unwrap();
    assert!(pending.is_empty());
    assert!(matches!(
        events.pop_front(),
        Some(AgentTurnEvent::PermissionResolved(id)) if id == "request"
    ));
    assert!(events.is_empty());
    assert_eq!(
        receive_echo(&mut transport).await,
        json!({"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    assert_eq!(
        receive_echo(&mut transport).await,
        json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"native"}})
    );

    let mut pending = retained();
    withdraw(&mut transport, &mut pending, &mut events)
        .await
        .unwrap();
    assert!(pending.is_empty());
    assert_eq!(events.len(), 1);
    assert_eq!(
        receive_echo(&mut transport).await["result"]["outcome"]["outcome"],
        "cancelled"
    );
    transport.close().await.unwrap();
}

#[tokio::test]
async fn respond_sends_one_validated_reply_and_keeps_rejected_answers_pending() {
    let mut transport = agent(&json!({}));
    let mut pending = retained();
    let mut events = VecDeque::new();
    for (id, response) in [
        ("missing", json!({"behavior":"allow"})),
        ("request", json!({"behavior":"unknown"})),
    ] {
        assert_eq!(
            respond(&mut transport, &mut pending, &mut events, id, &response).await,
            Err(AgentSessionError::Rejected)
        );
    }
    assert_eq!(pending.len(), 1);
    assert!(events.is_empty());
    respond(
        &mut transport,
        &mut pending,
        &mut events,
        "request",
        &json!({"behavior":"deny"}),
    )
    .await
    .unwrap();
    assert!(pending.is_empty());
    assert!(matches!(
        events.pop_front(),
        Some(AgentTurnEvent::PermissionResolved(id)) if id == "request"
    ));
    assert_eq!(
        receive_echo(&mut transport).await,
        json!({"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"selected","optionId":"no"}}})
    );
    transport.close().await.unwrap();
}

#[tokio::test]
async fn close_requests_session_close_only_when_the_agent_advertises_it() {
    let mut transport = agent(&json!({"session/close":{}}));
    close(
        &mut transport,
        "native",
        &json!({"sessionCapabilities":{"close":{}}}),
    )
    .await
    .unwrap();
    let frames = received(&mut transport);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "session/close");
    assert_eq!(frames[0]["params"], json!({"sessionId":"native"}));
    assert_eq!(
        transport.send(&json!({})).await,
        Err(AgentSessionError::Failed)
    );

    let mut transport = agent(&json!({}));
    close(&mut transport, "native", &json!({})).await.unwrap();
    assert!(received(&mut transport).is_empty());
    assert_eq!(
        transport.send(&json!({})).await,
        Err(AgentSessionError::Failed)
    );
}
