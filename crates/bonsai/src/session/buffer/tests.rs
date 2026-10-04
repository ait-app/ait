use super::Buffer;
use crate::event::{Body, Level, TEXT_CHUNK_BYTES, ToolKind, ToolState};

fn text(mid: &str, text: &str) -> Body {
    Body::Text {
        mid: mid.to_owned(),
        text: text.to_owned(),
    }
}

fn tool(id: &str, state: ToolState, output: Option<&str>) -> Body {
    Body::Tool {
        id: id.to_owned(),
        name: "Bash".to_owned(),
        state,
        kind: Some(ToolKind::Shell),
        title: Some("ls".to_owned()),
        input: Some("ls".to_owned()),
        output: output.map(str::to_owned),
        truncated: None,
    }
}

#[test]
fn same_message_text_merges_and_other_messages_do_not() {
    let mut buffer = Buffer::default();
    buffer.push(text("m1", "Hel"));
    buffer.push(text("m1", "lo"));
    buffer.push(text("m2", "x"));
    buffer.push(text("m1", "!"));
    assert_eq!(
        buffer.take(),
        vec![text("m1", "Hello"), text("m2", "x"), text("m1", "!")]
    );
    assert!(buffer.is_empty());
}

#[test]
fn adjacent_reasoning_merges() {
    let mut buffer = Buffer::default();
    buffer.push(Body::Reasoning {
        mid: None,
        text: "a".to_owned(),
    });
    buffer.push(Body::Reasoning {
        mid: None,
        text: "b".to_owned(),
    });
    assert_eq!(
        buffer.take(),
        vec![Body::Reasoning {
            mid: None,
            text: "ab".to_owned()
        }]
    );
}

#[test]
fn merged_text_never_exceeds_one_event() {
    let mut buffer = Buffer::default();
    let half = "y".repeat(TEXT_CHUNK_BYTES / 2 + 10);
    buffer.push(text("m", &half));
    buffer.push(text("m", &half));
    assert_eq!(buffer.take().len(), 2);
}

#[test]
fn tool_snapshots_keep_the_first_position_and_the_latest_fields() {
    let mut buffer = Buffer::default();
    buffer.push(tool("t1", ToolState::Running, None));
    buffer.push(text("m", "between"));
    buffer.push(tool("t1", ToolState::Running, Some("partial")));
    buffer.push(tool("t1", ToolState::Ok, Some("done")));
    let taken = buffer.take();
    assert_eq!(taken.len(), 2);
    assert_eq!(taken[0], tool("t1", ToolState::Ok, Some("done")));
    let mut sparse = Buffer::default();
    sparse.push(tool("t2", ToolState::Running, Some("out")));
    sparse.push(Body::Tool {
        id: "t2".to_owned(),
        name: "Bash".to_owned(),
        state: ToolState::Failed,
        kind: None,
        title: None,
        input: None,
        output: None,
        truncated: Some(true),
    });
    let merged = sparse.take();
    assert!(matches!(
        &merged[0],
        Body::Tool { state: ToolState::Failed, output: Some(o), truncated: Some(true), title: Some(_), .. } if o == "out"
    ));
}

#[test]
fn only_the_latest_usage_is_kept_and_other_events_append() {
    let mut buffer = Buffer::default();
    buffer.push(Body::Usage {
        input: Some(1),
        output: None,
        context: None,
    });
    buffer.push(Body::Notice {
        level: Level::Info,
        text: "n".to_owned(),
    });
    buffer.push(Body::Usage {
        input: Some(2),
        output: None,
        context: None,
    });
    assert_eq!(
        buffer.take(),
        vec![
            Body::Notice {
                level: Level::Info,
                text: "n".to_owned()
            },
            Body::Usage {
                input: Some(2),
                output: None,
                context: None
            },
        ]
    );
}
