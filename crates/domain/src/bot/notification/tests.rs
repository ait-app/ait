use serde_json::json;

use super::{
    LoggedNotification, Notification, NotificationKind, NotificationLog, SkipReason, Wake,
    WakeStatus,
};
use crate::{BotId, BotTurnId, ConversationId, TicketId};

fn notification() -> Notification {
    serde_json::from_value(json!({
        "id": "agent:a1:2026-11-12T08:00:00Z",
        "workspaceId": "wks_1",
        "kind": "agent_finished",
        "agentId": "a1",
        "title": "Fix login",
        "at": "2026-11-12T08:00:00Z"
    }))
    .unwrap()
}

#[test]
fn minimal_notification_json_defaults_optional_subjects_and_origin() {
    let loaded = notification();

    assert_eq!(loaded.kind, NotificationKind::AgentFinished);
    assert_eq!(loaded.scope, None);
    assert_eq!(loaded.schedule_id, None);
    assert_eq!(loaded.summary, None);
    assert_eq!(loaded.conversation_id, None);
    assert_eq!(loaded.ticket_id, None);
    let value = serde_json::to_value(&loaded).unwrap();
    assert_eq!(value["at"], "2026-11-12T08:00:00Z");
    assert!(value.get("ticketId").is_none() && value.get("ticketTitle").is_none());
}

#[test]
fn ticket_fields_round_trip() {
    let mut ticketed = notification();
    ticketed.ticket_id = Some(TicketId::generate());
    ticketed.ticket_title = Some("Fix login".into());
    ticketed.turn_started_at = Some(ticketed.at);

    let value = serde_json::to_value(&ticketed).unwrap();

    assert_eq!(value["ticketId"], json!(ticketed.ticket_id));
    assert_eq!(value["ticketTitle"], "Fix login");
    assert_eq!(value["turnStartedAt"], json!(ticketed.at));
    assert!(
        serde_json::to_value(notification())
            .unwrap()
            .get("turnStartedAt")
            .is_none()
    );
    assert_eq!(
        serde_json::from_value::<Notification>(value).unwrap(),
        ticketed
    );
}

#[test]
fn every_kind_and_wake_status_has_a_stable_wire_name() {
    for (kind, name) in [
        (NotificationKind::AgentFinished, "agent_finished"),
        (NotificationKind::AgentFailed, "agent_failed"),
        (NotificationKind::AgentPermission, "agent_permission"),
        (NotificationKind::ScheduleSucceeded, "schedule_succeeded"),
        (NotificationKind::ScheduleFailed, "schedule_failed"),
        (NotificationKind::TerminalAttention, "terminal_attention"),
        (NotificationKind::TicketBlocked, "ticket_blocked"),
        (NotificationKind::TicketDone, "ticket_done"),
        (NotificationKind::TicketFailed, "ticket_failed"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), name);
    }
    let conversation = ConversationId::generate();
    let turn = BotTurnId::generate();
    for (status, expected) in [
        (WakeStatus::Logged, json!({"state":"logged"})),
        (WakeStatus::Pending, json!({"state":"pending"})),
        (
            WakeStatus::Woke {
                conversation_id: conversation,
                turn_id: turn,
            },
            json!({"state":"woke","conversationId":conversation,"turnId":turn}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::OverBudget,
            },
            json!({"state":"skipped","reason":"over_budget"}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::InboxFull,
            },
            json!({"state":"skipped","reason":"inbox_full"}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::Paused,
            },
            json!({"state":"skipped","reason":"paused"}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::Observed,
            },
            json!({"state":"skipped","reason":"observed"}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::Blocked,
            },
            json!({"state":"skipped","reason":"blocked"}),
        ),
        (
            WakeStatus::Skipped {
                reason: SkipReason::Reported,
            },
            json!({"state":"skipped","reason":"reported"}),
        ),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<WakeStatus>(expected).unwrap(),
            status
        );
    }
}

#[test]
fn log_head_follows_the_next_sequence_and_records_roundtrip() {
    let mut log = NotificationLog::new(BotId::generate());
    assert_eq!(log.head(), 0);
    log.entries.push(LoggedNotification {
        sequence: 1,
        admitted_at: notification().at,
        notification: notification(),
        wake: WakeStatus::Pending,
    });
    log.next_sequence = 2;
    let wake = Wake {
        notifications: vec![notification()],
    };

    assert_eq!(log.head(), 1);
    assert_eq!(
        serde_json::from_value::<NotificationLog>(serde_json::to_value(&log).unwrap()).unwrap(),
        log
    );
    assert_eq!(
        serde_json::from_value::<Wake>(serde_json::to_value(&wake).unwrap()).unwrap(),
        wake
    );
}
