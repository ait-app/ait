use super::*;

use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use crate::protocol::timeline::FetchRequest;
use crate::rpc::timeline::fetch_stored;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;

async fn finish(manager: &mut AgentManager) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn inputs_precede_replies_in_live_storage_and_reopened_history() {
    let (directory, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let database = directory.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&database).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    let initial = manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    for id in ["first", "second"] {
        let prompt = AgentPrompt {
            text: "same question".to_owned(),
            client_message_id: Some(id.to_owned()),
            ..Default::default()
        };
        manager.send_input("agent", &prompt).await.unwrap();
        finish(&mut manager).await;
    }
    let (_, rows) = timeline.read("agent").unwrap();
    let mut speakers: Vec<_> = rows
        .iter()
        .filter_map(|row| match row.entry.item["type"].as_str() {
            Some("user_message" | "assistant_message") => {
                Some(row.entry.item["type"].as_str().unwrap())
            }
            _ => None,
        })
        .collect();
    speakers.dedup();
    assert_eq!(
        speakers,
        [
            "user_message",
            "assistant_message",
            "user_message",
            "assistant_message"
        ]
    );
    manager.close_all().await.unwrap();
    drop(manager);
    drop(timeline);

    let timeline = Timeline::open(&database).unwrap();
    let mut restored = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    restored.register_client(Box::new(client)).unwrap();
    let resumed = restored
        .restore("agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    assert_eq!(resumed.persistence, initial.persistence);
    restored
        .send_input(
            "agent",
            &AgentPrompt {
                text: "after restart".to_owned(),
                client_message_id: Some("third".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    finish(&mut restored).await;
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","direction":"tail","limit":30})).unwrap();
    let page = fetch_stored(&request, &timeline, "agent", &Value::Null, Some(false)).unwrap();
    let messages: Vec<_> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            matches!(
                entry["item"]["type"].as_str(),
                Some("user_message" | "assistant_message")
            )
        })
        .collect();
    assert_eq!(messages.len(), 6);
    for (pair, id) in messages
        .as_chunks::<2>()
        .0
        .iter()
        .zip(["first", "second", "third"])
    {
        assert_eq!(pair[0]["item"]["type"], "user_message");
        assert_eq!(pair[0]["item"]["clientMessageId"], id);
        assert_eq!(pair[1]["item"]["type"], "assistant_message");
        assert_eq!(pair[0]["turnId"], pair[1]["turnId"]);
        assert!(pair[0]["seqEnd"].as_u64().unwrap() < pair[1]["seqStart"].as_u64().unwrap());
    }
    restored.close_all().await.unwrap();
}

#[tokio::test]
async fn accepted_input_is_visible_before_native_failure_or_cancellation() {
    for text in ["crash-once", "wait"] {
        let (_directory, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        let prompt = AgentPrompt {
            text: text.to_owned(),
            client_message_id: Some("client-message".to_owned()),
            ..Default::default()
        };
        let turn = session.start_input(&prompt, &spec.config).await.unwrap();
        assert!(matches!(
            event(session.as_mut()).await.unwrap(),
            AgentTurnEvent::RuntimeInfo(_)
        ));
        let AgentTurnEvent::Timeline(input) = event(session.as_mut()).await.unwrap() else {
            panic!("accepted input missing")
        };
        assert_eq!(input.item["type"], "user_message");
        assert_eq!(input.item["text"], text);
        assert_eq!(input.item["clientMessageId"], "client-message");
        assert_eq!(input.turn_id.as_deref(), Some(turn.as_str()));
        if text == "wait" {
            session.cancel_turn(&turn).await.unwrap();
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejected_and_failed_writes_do_not_publish_input() {
    let (_directory, mut client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(session.start_turn("", &spec.config).await.is_err());
    assert_eq!(session.poll_turn().unwrap(), None);
    session.start_turn("accepted", &spec.config).await.unwrap();
    assert!(session.start_turn("busy", &spec.config).await.is_err());
    let events = completed(session.as_mut()).await;
    let inputs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => {
                Some(&entry.item)
            }
            _ => None,
        })
        .collect();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["text"], "accepted");
    assert!(inputs[0].get("clientMessageId").is_none());
    session.close().await.unwrap();

    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".to_owned(),
        "closed-input".to_owned(),
    )]))
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .start_turn("failed write", &spec.config)
            .await
            .is_err()
    );
    assert_eq!(session.poll_turn().unwrap(), None);
    session.close().await.unwrap();
}

#[tokio::test]
async fn text_attachments_are_recorded_in_native_submission_order() {
    let (directory, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        text: "question".to_owned(),
        attachments: vec![
            json!({"type":"text","mimeType":"text/plain","text":"context","contextKind":"chat_history"}),
            json!({"type":"text","mimeType":"text/plain","text":"notes"}),
        ],
        client_message_id: Some("rich-input".to_owned()),
        ..Default::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = completed(session.as_mut()).await;
    let inputs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => {
                Some(&entry.item)
            }
            _ => None,
        })
        .collect();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["text"], "context\nquestion\nnotes");
    assert_eq!(inputs[0]["clientMessageId"], "rich-input");
    let calls = logs(&directory);
    let sent = calls.iter().find_map(|call| call.get("input")).unwrap();
    assert_eq!(sent["message"]["content"], json!(prompt.blocks().unwrap()));
    session.close().await.unwrap();
}
