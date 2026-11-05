use std::path::Path;

use domain::{BotId, ConversationId};
use serde_json::json;

use super::*;
use crate::test_support::Native;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn conversation(bot: &BotId, session: &str) -> (ConversationId, String) {
    let id = ConversationId::generate();
    let record = json!({"id":id,"botId":bot,"channelId":"c","peer":"p","sender":"s","thread":null,
        "sessionId":session,"generation":1,"execution":{},"instructions":null,"nextSequence":3});
    (id, record.to_string())
}

fn closed(native: &Native) -> Vec<String> {
    let mut ids: Vec<String> = native
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, action)| match action {
            Action::Close { agent_id } => agent_id.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn legacy_store_sessions_are_closed_and_directory_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bots");
    let bot = BotId::generate();
    write(
        &root.join("catalog.json"),
        &json!({"bots":[{"id":bot,"workspaceId":"workspace-1","name":"n","enabled":true}],"channels":[]})
            .to_string(),
    );
    for session in ["session-a", "session-b"] {
        let (id, record) = conversation(&bot, session);
        write(&root.join(format!("conversations/{id}.json")), &record);
    }
    let native = Native::default();
    discard(&root, &native).await;
    assert_eq!(closed(&native), ["session-a", "session-b"]);
    assert!(
        native
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(owner, _)| owner.workspace_id == "workspace-1" && owner.bot_id == bot)
    );
    assert!(!root.exists());
}

#[tokio::test]
async fn legacy_store_with_malformed_records_is_discarded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bots");
    write(&root.join("catalog.json"), "{not json");
    let bot = BotId::generate();
    let (id, record) = conversation(&bot, "session-a");
    write(&root.join(format!("conversations/{id}.json")), &record);
    write(&root.join("conversations/truncated.json"), &record[..20]);
    let native = Native::default();
    discard(&root, &native).await;
    assert!(closed(&native).is_empty());
    assert!(!root.exists());

    let root = dir.path().join("partial");
    write(
        &root.join("catalog.json"),
        &json!({"bots":[{"id":bot,"workspaceId":"workspace-1"}]}).to_string(),
    );
    write(&root.join(format!("conversations/{id}.json")), &record);
    write(&root.join("conversations/truncated.json"), &record[..20]);
    discard(&root, &native).await;
    assert_eq!(closed(&native), ["session-a"]);
    assert!(!root.exists());
}

#[tokio::test]
async fn missing_legacy_store_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let native = Native::default();
    discard(&dir.path().join("bots"), &native).await;
    assert!(native.calls.lock().unwrap().is_empty());
}
