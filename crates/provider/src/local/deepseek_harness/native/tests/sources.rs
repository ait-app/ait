use super::fixture::{Fixture, next};
use crate::{
    ports::agent_session::{AgentClient, AgentTurnEvent},
    storage::timeline::Timeline,
};
use serde_json::json;

#[tokio::test]
async fn live_and_recovered_history_only_project_human_input() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = session.persistence().unwrap();
    let text = "Current runtime context. Current DSH file policy: read-only.";
    for (index, source) in [
        json!({"kind":"plugin","plugin":"@deepseek-ai/dsh-system-prompt"}),
        json!({"kind":"goal"}),
        json!({"kind":"future-context-kind"}),
        json!(null),
    ]
    .into_iter()
    .enumerate()
    {
        fixture.history(
            index as u64 + 1,
            "user/message",
            json!({"id":format!("context-{index}"),"source":source,"content":[
                {"type":"text","text":text},
                {"type":"image","attachment":{"attachmentId":"private-context"}}
            ]}),
        );
    }
    fixture.history(
        5,
        "user/message",
        json!({
            "id":"human", "source":{"kind":"user","rpcId":"input-request"},
            "content":[{"type":"text","text":text}]
        }),
    );
    let AgentTurnEvent::Timeline(user) = next(session.as_mut()).await else {
        panic!("only human input should be displayed")
    };
    assert_eq!(user.item["messageId"], "human");
    assert_eq!(user.item["clientMessageId"], "input-request");
    assert_eq!(user.item["text"], text, "do not filter by text prefixes");
    session.close().await.unwrap();
    let recovered = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(&recovered).unwrap(), json!([user]));
    assert!(fixture.requests("session/attachment").is_empty());
    assert!(fixture.requests("session/prompt").is_empty());

    // Repair already persisted context rows, then keep the repaired epoch stable.
    let path = std::path::Path::new(&fixture.spec.cwd).join("timeline.sqlite3");
    let timeline = Timeline::open(&path).unwrap();
    let mut context = recovered[0].clone();
    context.key = "native:dsh:v2:session:user:context-0".into();
    context.item["messageId"] = json!("context-0");
    context
        .item
        .as_object_mut()
        .unwrap()
        .remove("clientMessageId");
    timeline
        .append(
            "agent",
            "deepseek-harness",
            &[context, recovered[0].clone()],
        )
        .unwrap();
    let old_epoch = timeline.read("agent").unwrap().0;
    let repaired = timeline
        .reconcile("agent", "deepseek-harness", &recovered)
        .unwrap();
    assert_ne!(old_epoch, repaired);
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();
    for _ in 0..2 {
        assert_eq!(
            timeline
                .reconcile("agent", "deepseek-harness", &recovered)
                .unwrap(),
            repaired
        );
        let (_, rows) = timeline.read("agent").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entry.item["messageId"], "human");
    }
}
