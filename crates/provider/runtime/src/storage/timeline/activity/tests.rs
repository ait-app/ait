use chrono::{DateTime, Utc};
use provider_core::protocol::timeline::NativeItem;
use serde_json::json;

use super::parse_timestamp;
use crate::storage::timeline::Timeline;

fn entry(key: &str, kind: &str, timestamp: &str) -> NativeItem {
    NativeItem {
        key: key.to_owned(),
        turn_id: None,
        timestamp: timestamp.to_owned(),
        item: json!({"type":kind,"text":key}),
    }
}

fn at(value: &str) -> DateTime<Utc> {
    parse_timestamp(value).unwrap()
}

#[test]
fn finds_the_newest_user_message_after_the_cutoff() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "claude",
            &[
                entry("old", "user_message", "2026-11-18T10:00:00.000Z"),
                entry("first", "user_message", "2026-11-18T10:01:00+00:00"),
                entry("second", "user_message", "2026-11-18T10:02:00.500000+00:00"),
                entry("reply", "assistant_message", "2026-11-18T10:03:00.000Z"),
            ],
        )
        .unwrap();

    let found = timeline
        .latest_user_message_after("agent", at("2026-11-18T10:00:30Z"))
        .unwrap();

    assert_eq!(found, Some(at("2026-11-18T10:02:00.500Z")));
}

#[test]
fn stops_at_the_cutoff_without_reading_older_messages() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "claude",
            &[
                entry("question", "user_message", "2026-11-18T10:00:00.000Z"),
                entry("answer", "assistant_message", "2026-11-18T10:05:00.000Z"),
            ],
        )
        .unwrap();

    let after_answer = timeline
        .latest_user_message_after("agent", at("2026-11-18T10:05:00Z"))
        .unwrap();
    let before_answer = timeline
        .latest_user_message_after("agent", at("2026-11-18T10:01:00Z"))
        .unwrap();

    assert_eq!(after_answer, None);
    assert_eq!(before_answer, None);
    assert_eq!(
        timeline
            .latest_user_message_after("missing", at("2026-11-18T09:00:00Z"))
            .unwrap(),
        None
    );
}

#[test]
fn skips_reappended_plugin_rows_and_unreadable_timestamps() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "claude",
            &[
                entry("question", "user_message", "2026-11-18T10:02:00.000Z"),
                entry("unstamped", "assistant_message", "not a time"),
                entry("plugin", "plugin", "2026-11-18T09:00:00.000Z"),
            ],
        )
        .unwrap();

    let found = timeline
        .latest_user_message_after("agent", at("2026-11-18T10:00:00Z"))
        .unwrap();

    assert_eq!(found, Some(at("2026-11-18T10:02:00Z")));
}
