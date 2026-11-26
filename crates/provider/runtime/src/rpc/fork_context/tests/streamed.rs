//! Boundaries sent by live clients for text items that were streamed before they were committed.

use super::*;
use crate::storage::timeline::Timeline;

const TURN: &str = "turn-1";

fn native(seq: u64, id: &str, item: Value) -> Row {
    let mut result = row(seq, item);
    result.entry.key = format!("native:{TURN}:{id}");
    result.entry.turn_id = Some(TURN.to_owned());
    result
}

/// Rows as `progress::read` normalizes one streamed item: each delta keeps its text, and the
/// committed row after them keeps only the suffix that was not streamed, which here is empty.
fn streamed(first: u64, id: &str, kind: &str, deltas: &[&str]) -> Vec<Row> {
    deltas
        .iter()
        .chain([&""])
        .zip(first..)
        .map(|(text, seq)| {
            let mut item = json!({"type":kind,"text":text});
            if kind == "assistant_message" {
                item["messageId"] = json!(id);
            }
            native(seq, id, item)
        })
        .collect()
}

/// A question at 1 and its reply streamed at 2..=4, committed at 5 with an empty suffix.
fn completed_reply() -> Vec<Row> {
    let mut rows = vec![native(
        1,
        "user",
        json!({"type":"user_message","text":"Question"}),
    )];
    rows.extend(streamed(
        2,
        "reply",
        "assistant_message",
        &["Hel", "lo ", "world"],
    ));
    rows
}

fn boundary(rows: &[Row], seq: u64, message: Option<&str>) -> Result<Option<u64>, ErrorCode> {
    let mut request = request();
    request.boundary_cursor = Some(Cursor {
        epoch: "epoch".to_owned(),
        seq,
    });
    resolve_boundary(&request, message, "epoch", &project(rows))
}

#[test]
fn stored_reply_forks_from_a_live_cursor_one_before_its_empty_completion() {
    let timeline = Timeline::memory().unwrap();
    let item = |turn: &str, id: &str, item: Value| NativeItem {
        key: format!("native:{turn}:{id}"),
        turn_id: Some(turn.to_owned()),
        timestamp: "time".to_owned(),
        item,
    };
    let reply = |text: &str| {
        item(
            TURN,
            "reply",
            json!({"type":"assistant_message","messageId":"reply","text":text}),
        )
    };
    let question = item(
        TURN,
        "user",
        json!({"type":"user_message","text":"Question"}),
    );
    timeline.append("agent", "codex", &[question]).unwrap();
    for (observation, delta) in [("one", "Hel"), ("two", "lo "), ("three", "world")] {
        timeline
            .progress("agent", "codex", observation, &reply(delta))
            .unwrap();
    }
    let (epoch, committed) = timeline
        .append("agent", "codex", &[reply("Hello world")])
        .unwrap();
    let later = item(
        "turn-2",
        "user",
        json!({"type":"user_message","text":"Later question"}),
    );
    timeline.append("agent", "codex", &[later]).unwrap();
    let (_, rows) = timeline.read("agent").unwrap();
    assert_eq!(committed, [5]);
    assert_eq!(rows[4].entry.item["text"], "");
    let mut request = request();
    request.boundary_cursor = Some(Cursor {
        epoch: epoch.clone(),
        seq: 4,
    });
    request.boundary_message_id = Some("reply".to_owned());

    let exported = export(&request, &epoch, &rows, &json!({})).unwrap();

    let text = exported["attachment"]["text"].as_str().unwrap();
    assert!(
        text.ends_with("[User] Question\n[Assistant] Hello world\n</chat-history-summary>"),
        "{text}"
    );
    assert_eq!(exported["itemCount"], 2);
}

#[test]
fn live_cursor_one_before_an_empty_completion_resolves_to_the_completion() {
    let rows = completed_reply();
    let mut request = request();
    request.boundary_cursor = Some(Cursor {
        epoch: "epoch".to_owned(),
        seq: 4,
    });
    request.boundary_message_id = Some("reply".to_owned());

    let exported = export(&request, "epoch", &rows, &json!({})).unwrap();

    let text = exported["attachment"]["text"].as_str().unwrap();
    assert!(
        text.contains("[User] Question\n[Assistant] Hello world\n"),
        "{text}"
    );
    assert_eq!(boundary(&rows, 4, Some("reply")), Ok(Some(5)));
    assert_eq!(boundary(&rows, 4, None), Ok(Some(5)));
}

#[test]
fn cursor_at_the_first_fragment_resolves_to_the_end_of_the_reply() {
    assert_eq!(boundary(&completed_reply(), 2, None), Ok(Some(5)));
}

#[test]
fn cursor_inside_another_message_resolves_through_the_requested_message_id() {
    let mut rows = completed_reply();
    rows.extend(streamed(6, "next", "assistant_message", &["Next"]));

    assert_eq!(boundary(&rows, 3, Some("next")), Ok(Some(7)));
    assert_eq!(
        boundary(&rows, 3, Some("missing")),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn cursor_inside_a_tool_lifecycle_resolves_only_through_a_message_id() {
    let tool = |seq, status, label| {
        native(
            seq,
            "exec",
            json!({"type":"tool_call","callId":"exec","name":"terminal","status":status,
                "detail":{"type":"plain_text","label":label}}),
        )
    };
    let mut rows = vec![
        native(1, "user", json!({"type":"user_message","text":"Run it"})),
        tool(2, "running", "started"),
    ];
    rows.extend(streamed(
        3,
        "commentary",
        "assistant_message",
        &["Waiting."],
    ));
    rows.push(tool(5, "completed", "finished"));

    assert_eq!(boundary(&rows, 2, None), Err(ErrorCode::InvalidMessage));
    assert_eq!(boundary(&rows, 2, Some("commentary")), Ok(Some(4)));
}

#[test]
fn cursor_from_a_replaced_epoch_is_rejected_even_with_a_valid_message_id() {
    let mut request = request();
    request.boundary_cursor = Some(Cursor {
        epoch: "replaced".to_owned(),
        seq: 5,
    });
    request.boundary_message_id = Some("reply".to_owned());

    assert_eq!(
        export(&request, "epoch", &completed_reply(), &json!({})),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn cursor_inside_streamed_reasoning_resolves_to_its_end_unless_a_message_is_named() {
    let mut rows = vec![native(
        1,
        "user",
        json!({"type":"user_message","text":"Question"}),
    )];
    rows.extend(streamed(2, "thinking", "reasoning", &["Think", "ing"]));
    rows.extend(streamed(5, "reply", "assistant_message", &["Answer"]));

    assert_eq!(boundary(&rows, 3, None), Ok(Some(4)));
    assert_eq!(boundary(&rows, 3, Some("reply")), Ok(Some(6)));
}
