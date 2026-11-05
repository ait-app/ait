use super::*;
use domain::bot::{Channel, ChannelConfig, DeliveryStatus, TurnStatus};
use domain::{BotId, BotTurnId, ChannelId};

#[test]
fn atomic_catalog_routes_receipts_and_bounded_memory_survive_reopening() {
    let root = tempfile::tempdir().unwrap();
    let store = FileStore::new(root.path().join("store"));
    assert_eq!(store.catalog().unwrap(), Catalog::default());
    assert!(store.conversations().unwrap().is_empty());
    let bot = BotId::generate();
    let channel = ChannelId::generate();
    let catalog = Catalog {
        bots: Vec::new(),
        channels: vec![Channel {
            id: channel,
            bot_id: bot,
            config: ChannelConfig::Builtin,
            enabled: true,
            cursor: None,
            deleted: false,
        }],
    };
    store.save_catalog(&catalog).unwrap();
    let conversation = Conversation {
        id: ConversationId::generate(),
        bot_id: bot,
        channel_id: channel,
        peer: "owner".into(),
        sender: "owner".into(),
        thread: None,
        session_id: BotId::generate().to_string(),
        generation: 0,
        execution: domain::bot::ExecutionConfig {
            provider: "test".into(),
            model: None,
            mode_id: None,
            thinking: None,
        },
        instructions: String::new(),
        next_sequence: 2,
    };
    store.save_conversation(&conversation).unwrap();
    let turn = Turn {
        id: BotTurnId::generate(),
        conversation_id: conversation.id,
        message_id: "one".into(),
        sequence: 1,
        generation: 0,
        text: "hello".into(),
        reply_to: None,
        reply: Some("world".into()),
        pending_permissions: Vec::new(),
        status: TurnStatus::Completed,
        delivery: DeliveryStatus::Delivered,
        error: None,
        created_at: "2026-10-12T00:00:00Z".into(),
    };
    store.save_turn(&turn).unwrap();
    fs::write(
        root.path()
            .join("store/turns")
            .join(conversation.id.to_string())
            .join(".atomic-write-in-progress"),
        b"{",
    )
    .unwrap();
    assert_eq!(store.memory("workspace-1").unwrap(), "");
    store.save_memory("workspace-1", "remember").unwrap();
    assert_eq!(store.memory("workspace-1").unwrap(), "remember");
    assert_eq!(store.memory("workspace-2").unwrap(), "");
    assert!(
        store
            .save_memory("workspace-1", &"a".repeat(32769))
            .is_err()
    );
    let reopened = FileStore::new(root.path().join("store"));
    assert_eq!(reopened.catalog().unwrap(), catalog);
    assert_eq!(
        reopened.conversations().unwrap(),
        vec![conversation.clone()]
    );
    assert_eq!(reopened.turns(conversation.id).unwrap(), vec![turn.clone()]);
    assert_eq!(reopened.turn(conversation.id, turn.id).unwrap(), turn);
}

#[test]
fn memory_is_stored_under_the_store_root_by_workspace() {
    let root = tempfile::tempdir().unwrap();
    let store = FileStore::new(root.path().join("workspace-bots"));
    assert_eq!(store.memory("wks_a").unwrap(), "");
    store.save_memory("wks_a", "PR titles in Chinese").unwrap();
    assert_eq!(store.memory("wks_a").unwrap(), "PR titles in Chinese");
    assert_eq!(store.memory("wks_b").unwrap(), "");
    let file = root
        .path()
        .join("workspace-bots/memory")
        .join(format!("{:x}.md", Sha256::digest(b"wks_a")));
    assert!(file.is_file());
}

#[test]
fn delete_memory_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let store = FileStore::new(root.path().join("workspace-bots"));
    store.delete_memory("wks_a").unwrap();
    store.save_memory("wks_a", "x").unwrap();
    store.delete_memory("wks_a").unwrap();
    assert_eq!(store.memory("wks_a").unwrap(), "");
}

#[test]
fn memory_rejects_oversize_and_nul() {
    let root = tempfile::tempdir().unwrap();
    let store = FileStore::new(root.path().join("workspace-bots"));
    assert!(
        store
            .save_memory("wks_a", &"x".repeat(32 * 1024 + 1))
            .is_err()
    );
    assert!(store.save_memory("wks_a", "a\0b").is_err());
}
