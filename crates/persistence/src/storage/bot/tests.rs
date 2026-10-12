use super::*;
use domain::bot::{Channel, ChannelConfig, DeliveryStatus, TurnStatus};
use domain::{BotTurnId, ChannelId};

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
    let cwd = root.path().to_str().unwrap();
    assert_eq!(store.memory(cwd, "workspace-1", bot).unwrap(), "");
    store
        .save_memory(cwd, "workspace-1", bot, "remember")
        .unwrap();
    assert_eq!(store.memory(cwd, "workspace-1", bot).unwrap(), "remember");
    assert_eq!(store.memory(cwd, "workspace-2", bot).unwrap(), "");
    assert!(
        store
            .save_memory(cwd, "workspace-1", bot, &"a".repeat(32769))
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

#[cfg(unix)]
#[test]
fn memory_rejects_symlink_directory_and_file_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FileStore::new(root.path().join("store"));
    let bot = BotId::generate();
    let cwd = root.path().to_str().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".ait")).unwrap();
    assert!(store.save_memory(cwd, "workspace", bot, "escape").is_err());
    assert!(store.memory(cwd, "workspace", bot).is_err());
    fs::remove_file(root.path().join(".ait")).unwrap();
    store.save_memory(cwd, "workspace", bot, "local").unwrap();
    let file = root.path().join(model::bot::memory_path("workspace", bot));
    fs::remove_file(&file).unwrap();
    let target = outside.path().join("outside.md");
    fs::write(&target, "private").unwrap();
    std::os::unix::fs::symlink(&target, &file).unwrap();
    assert!(store.memory(cwd, "workspace", bot).is_err());
    store
        .save_memory(cwd, "workspace", bot, "replacement")
        .unwrap();
    assert_eq!(fs::read_to_string(target).unwrap(), "private");
}
