use crate::{BotId, BotTurnId, ChannelId, ConversationId};

#[test]
fn bot_identities_roundtrip_and_reject_nil_or_malformed_values() {
    let bot = BotId::generate();
    assert_eq!(
        serde_json::from_str::<BotId>(&serde_json::to_string(&bot).unwrap()).unwrap(),
        bot
    );
    assert_eq!(
        serde_json::from_str::<ChannelId>(&serde_json::to_string(&ChannelId::generate()).unwrap())
            .unwrap()
            .to_string()
            .len(),
        36
    );
    assert_eq!(
        serde_json::from_str::<ConversationId>(
            &serde_json::to_string(&ConversationId::generate()).unwrap()
        )
        .unwrap()
        .to_string()
        .len(),
        36
    );
    assert_eq!(
        serde_json::from_str::<BotTurnId>(&serde_json::to_string(&BotTurnId::generate()).unwrap())
            .unwrap()
            .to_string()
            .len(),
        36
    );
    for invalid in [
        "\"00000000-0000-0000-0000-000000000000\"",
        "\"../escape\"",
        "1",
    ] {
        assert!(serde_json::from_str::<BotId>(invalid).is_err());
    }
}

#[test]
fn bot_state_serializes_as_kind_tagged_snake_case() {
    use super::{BotState, PauseReason};
    use serde_json::json;

    for (state, expected) in [
        (BotState::Active, json!({"kind":"active"})),
        (
            BotState::Paused {
                reason: PauseReason::User,
            },
            json!({"kind":"paused","reason":"user"}),
        ),
        (
            BotState::Paused {
                reason: PauseReason::DirectoryMissing,
            },
            json!({"kind":"paused","reason":"directory_missing"}),
        ),
        (
            BotState::Retired {
                at: "2026-11-05T00:00:00Z".into(),
            },
            json!({"kind":"retired","at":"2026-11-05T00:00:00Z"}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&state).unwrap(), expected);
        assert_eq!(serde_json::from_value::<BotState>(expected).unwrap(), state);
    }
    assert!(serde_json::from_value::<BotState>(json!({"kind":"enabled"})).is_err());
}

#[test]
fn phase_two_bot_json_loads_with_default_permissions() {
    let json = serde_json::json!({
        "id": BotId::generate(),
        "workspaceId": "w1",
        "execution": {"provider": "codex", "model": null, "modeId": null, "thinking": null},
        "builtinChannelId": ChannelId::generate(),
        "state": {"kind": "active"},
        "createdAt": "2026-01-01T00:00:00Z",
    });
    let bot: super::Bot = serde_json::from_value(json).unwrap();
    assert_eq!(
        bot.permissions,
        super::permissions::BotPermissions::default()
    );
    assert!(bot.workspaces.is_empty());
    let value = serde_json::to_value(&bot).unwrap();
    assert_eq!(
        value["permissions"],
        serde_json::json!({"groups": ["observe", "agents", "stop", "schedules"], "modes": {}})
    );
}

#[test]
fn phase_two_conversation_json_loads_without_an_instructions_digest() {
    let json = serde_json::json!({
        "id": ConversationId::generate(),
        "botId": BotId::generate(),
        "channelId": ChannelId::generate(),
        "peer": "owner",
        "sender": "owner",
        "thread": null,
        "sessionId": "session-1",
        "generation": 0,
        "execution": {"provider": "codex", "model": null, "modeId": null, "thinking": null},
        "nextSequence": 1,
    });

    let conversation: super::Conversation = serde_json::from_value(json).unwrap();

    assert_eq!(conversation.instructions_digest, None);
}

#[test]
fn turn_messages_roundtrip_and_old_turns_default_to_empty_messages() {
    use super::{DeliveryStatus, Turn, TurnMessage, TurnMessageStatus, TurnStatus};

    let id = BotTurnId::generate();
    let conversation_id = ConversationId::generate();
    let message = TurnMessage {
        id: "assistant-1".into(),
        text: "first answer".into(),
        status: TurnMessageStatus::Streaming,
    };
    let value = serde_json::json!({
        "id": id,
        "conversationId": conversation_id,
        "messageId": "input-1",
        "sequence": 1,
        "generation": 0,
        "text": "question",
        "replyTo": null,
        "reply": "first answer",
        "pendingPermissions": [],
        "status": "running",
        "delivery": "pending",
        "error": null,
        "createdAt": "2026-11-05T00:00:00Z"
    });
    let mut turn: Turn = serde_json::from_value(value).unwrap();
    assert_eq!(turn.messages, Vec::<TurnMessage>::new());
    turn.messages.push(message.clone());
    assert_eq!(
        serde_json::from_value::<TurnMessage>(serde_json::to_value(message).unwrap()).unwrap(),
        TurnMessage {
            id: "assistant-1".into(),
            text: "first answer".into(),
            status: TurnMessageStatus::Streaming,
        }
    );
    assert_eq!(turn.status, TurnStatus::Running);
    assert_eq!(turn.delivery, DeliveryStatus::Pending);
}
