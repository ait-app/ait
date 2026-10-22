//! Full Host assembly -> journal -> existing serve WSS, with an isolated center fixture.

use std::collections::BTreeMap;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use uuid::Uuid;

use crate::host::{Server, tests::config};

fn frame(header: &Value, payload: &Value) -> Message {
    let header = serde_json::to_vec(header).unwrap();
    let mut bytes = u16::try_from(header.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend(header);
    bytes.extend(serde_json::to_vec(payload).unwrap());
    Message::Binary(bytes.into())
}

fn seed_attention(directory: &std::path::Path) {
    let registry = persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
        directory.join("agents/agents.json"),
    );
    registry
        .upsert(
            &serde_json::from_value(json!({
                "id":"pending-agent","provider":"test","cwd":directory,
                "createdAt":"now","updatedAt":"now",
                "workspaceId":"workspace","title":"Completed task","requiresAttention":true,
                "attentionReason":"finished","attentionTimestamp":chrono::Utc::now().to_rfc3339(),
                "config":{"systemPrompt":"must-stay-local"}
            }))
            .unwrap(),
        )
        .unwrap();
}

#[tokio::test]
async fn real_host_publishes_current_attention_and_three_directories_over_serve_link() {
    let directory = tempfile::tempdir().unwrap();
    seed_attention(directory.path());
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = config(directory.path());
    config.hub_notifications = true;
    let server = Server::bind(config).await.unwrap();
    let address = server.address();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let host = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let response = reqwest::Client::new()
        .put(format!("http://{address}/api/hub/control"))
        .bearer_auth("offline-host-test-token-at-least-32-characters")
        .json(
            &json!({"center_url":format!("http://{}",center.local_addr().unwrap()),
            "hub_ticket":"a".repeat(64),"node_session_id":Uuid::new_v4()}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let (tcp, _) = center.accept().await.unwrap();
    let mut socket = accept_async(tcp).await.unwrap();
    assert!(matches!(socket.next().await, Some(Ok(Message::Binary(_)))));
    socket
        .send(frame(
            &json!({
                "t":"welcome","link":Uuid::new_v4(),"resumed":false,"recv":0,
                "resume_token":"b".repeat(64),"grace_seconds":120,
                "center_methods":["hub.events.publish","hub.directory.publish"]
            }),
            &Value::Null,
        ))
        .await
        .unwrap();
    let published = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut published = BTreeMap::new();
        let mut sequence = 0;
        while published.len() < 4 {
            let Message::Binary(bytes) = socket.next().await.unwrap().unwrap() else {
                continue;
            };
            let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
            let header: Value = serde_json::from_slice(&bytes[2..2 + length]).unwrap();
            if header["t"] != "call" {
                continue;
            }
            let params: Value = serde_json::from_slice(&bytes[2 + length..]).unwrap();
            let result = if header["method"] == "hub.events.publish" {
                assert_eq!(params["method"], "agent_attention_required");
                published.insert("attention".to_owned(), params);
                json!({"cursor":1,"inserted":true})
            } else {
                assert_eq!(header["method"], "hub.directory.publish");
                let result = json!({"revision":params["revision"],"changed":true});
                published.insert(params["kind"].as_str().unwrap().to_owned(), params);
                result
            };
            sequence += 1;
            socket
                .send(frame(
                    &json!({"t":"reply","seq":sequence,"id":header["id"]}),
                    &json!({"result":result}),
                ))
                .await
                .unwrap();
        }
        published
    })
    .await
    .unwrap();
    assert_eq!(
        published["attention"]["notification"]["title"],
        "Agent finished"
    );
    assert_eq!(published["agent"]["items"][0]["id"], "pending-agent");
    assert_eq!(published["project"]["items"], json!([]));
    assert_eq!(published["workspace"]["items"], json!([]));
    assert!(
        !serde_json::to_string(&published)
            .unwrap()
            .contains("must-stay-local")
    );
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), host)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(directory.path().join("hub/publications.json").exists());
}
