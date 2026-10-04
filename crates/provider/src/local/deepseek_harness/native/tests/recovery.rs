use super::fixture::{Fixture, next};
use crate::{
    ports::agent_session::{AgentClient, AgentTurnEvent},
    protocol::{prompt::AgentPrompt, timeline::NativeItem},
    storage::timeline::Timeline,
};
use serde_json::{Value, json};

#[tokio::test]
async fn native_argument_validation_failures_do_not_fail_the_adapter_or_lose_user_messages() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = session.persistence().unwrap();
    let mut live = Vec::new();
    for (index, raw) in ["", "{broken", "   ", "null", "[]", "{\"value\":1}"]
        .iter()
        .enumerate()
    {
        let turn = index as u64 + 1;
        let seq = index as u64 * 6;
        let prompt = AgentPrompt {
            text: format!("question {index}"),
            client_message_id: Some(format!("client-{index}")),
            ..AgentPrompt::default()
        };
        session
            .start_input(&prompt, &fixture.spec.config)
            .await
            .unwrap();
        let _ = next(session.as_mut()).await;
        assert_eq!(
            fixture.requests("session/prompt")[index]["request"]["requestId"],
            format!("client-{index}")
        );
        fixture.history(seq+1,"user/message",json!({"id":format!("user-{index}"),"content":[{"type":"text","text":prompt.text}],"source":{"rpcId":format!("client-{index}")}}));
        let AgentTurnEvent::Timeline(user) = next(session.as_mut()).await else {
            panic!("user must be durable before assistant")
        };
        assert_eq!(user.item["clientMessageId"], format!("client-{index}"));
        live.push(user);
        fixture.history(seq + 2, "turn/start", json!({"turn":turn}));
        fixture.history(
            seq + 3,
            "tool/call",
            json!({"turn":turn,"callId":"reused-call-id","name":"test","arguments":raw}),
        );
        let AgentTurnEvent::Progress { entry, .. } = next(session.as_mut()).await else {
            panic!("tool progress")
        };
        let expected = if raw.is_empty() {
            json!({})
        } else {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!(raw))
        };
        assert_eq!(entry.item["detail"]["input"], expected);
        fixture.history(seq+4,"tool/result",json!({"turn":turn,"message":{"content":[{"type":"tool-result","toolCallId":"reused-call-id","isError":true,"content":[{"type":"text","text":"native validation rejected arguments"}]}]}}));
        let AgentTurnEvent::Timeline(tool) = next(session.as_mut()).await else {
            panic!("native tool failure must be displayed")
        };
        live.push(tool);
        fixture.history(seq+5,"assistant/message",json!({"turn":turn,"message":{"id":format!("answer-{index}"),"content":[{"type":"text","text":"I can correct the arguments"}]}}));
        let _ = next(session.as_mut()).await;
        let AgentTurnEvent::Timeline(answer) = next(session.as_mut()).await else {
            panic!("assistant")
        };
        live.push(answer);
        fixture.history(
            seq + 6,
            "turn/end",
            json!({"turn":turn,"reason":{"kind":"completed"}}),
        );
        assert!(matches!(
            next(session.as_mut()).await,
            AgentTurnEvent::Completed(_)
        ));
    }
    session.close().await.unwrap();
    assert!(fixture.client.supports_history_replay());
    let replay = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&live).unwrap(),
        serde_json::to_value(&replay).unwrap()
    );
    assert_eq!(
        fixture.requests("session/create").len(),
        1,
        "history must not create or resume a writer"
    );
    assert_eq!(
        fixture.requests("session/prompt").len(),
        6,
        "history must not replay input"
    );
    assert!(
        fixture.requests("session/page").len() > 1,
        "older messages must be paged in"
    );
    recover_legacy_and_reopen(&fixture, &replay);
}

fn recover_legacy_and_reopen(fixture: &Fixture, replay: &[NativeItem]) {
    let path = std::path::Path::new(&fixture.spec.cwd).join("timeline.sqlite3");
    let timeline = Timeline::open(&path).unwrap();
    let mut old = replay
        .iter()
        .filter(|entry| entry.item["type"] != "user_message")
        .cloned()
        .collect::<Vec<_>>();
    for (index, entry) in old.iter_mut().enumerate() {
        entry.key = format!("native:legacy:{index}");
    }
    timeline.append("agent", "deepseek-harness", &old).unwrap();
    let old_epoch = timeline.read("agent").unwrap().0;
    let repaired = timeline
        .reconcile("agent", "deepseek-harness", replay)
        .unwrap();
    assert_ne!(old_epoch, repaired);
    assert_eq!(
        timeline
            .reconcile("agent", "deepseek-harness", replay)
            .unwrap(),
        repaired
    );
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();
    let (epoch, rows) = timeline.read("agent").unwrap();
    assert_eq!(epoch, repaired);
    assert_eq!(rows.len(), 18);
    assert_eq!(
        rows.iter()
            .filter(|row| row.entry.item["type"] == "user_message")
            .count(),
        6
    );
    for chunk in rows.as_chunks::<3>().0 {
        assert_eq!(chunk[0].entry.item["type"], "user_message");
        assert_eq!(chunk[1].entry.item["type"], "tool_call");
        assert_eq!(chunk[2].entry.item["type"], "assistant_message");
    }
}

#[tokio::test]
async fn incomplete_native_history_cannot_replace_existing_display_history() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = session.persistence().unwrap();
    fixture.history(
        2,
        "user/message",
        json!({"id":"gap","content":[{"type":"text","text":"missing event one"}]}),
    );
    session.close().await.unwrap();
    assert!(
        fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .is_err()
    );
    assert!(fixture.requests("session/prompt").is_empty());
}

#[tokio::test]
async fn queued_user_images_and_files_survive_recovery_without_resubmitting_input() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = session.persistence().unwrap();
    fixture.history(1, "user/message", json!({
        "id": "queued-user",
        "content": [
            {"type": "text", "text": "explain these attachments"},
            {"type": "image", "attachment": {"attachmentId": "image", "mediaType": "image/png"}},
            {"type": "file", "attachment": {"name": "notes.txt"}}
        ]
    }));
    let AgentTurnEvent::Timeline(user) = next(session.as_mut()).await else {
        panic!("queued user input must be durable even outside an active turn")
    };
    assert_eq!(user.item["type"], "user_message");
    let text = user.item["text"].as_str().unwrap();
    assert!(text.starts_with("explain these attachments\n"));
    assert!(text.contains("!["));
    assert!(text.ends_with("[Attachment: notes.txt]"));
    assert!(user.item.get("clientMessageId").is_none());
    session.close().await.unwrap();
    let replay = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), json!([user]));
    assert_eq!(fixture.requests("session/attachment").len(), 2);
    assert!(fixture.requests("session/prompt").is_empty());
    let legacy = fixture.client.clone().with_acp_profile();
    assert!(!legacy.supports_history_replay());
    assert!(legacy.history(&handle, &fixture.spec.cwd).await.is_err());
}
