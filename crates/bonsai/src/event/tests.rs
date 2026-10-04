use serde_json::{Value, json};

use super::{
    AskKind, AskOption, Body, ContextUsage, Effect, Event, Origin, TEXT_CHUNK_BYTES, ToolKind,
    ToolState, chunks, field, fit,
};
use crate::wire::{EVENT_BYTES, FIELD_BYTES};

fn value(body: Body) -> Value {
    serde_json::to_value(Event {
        seq: 3,
        at: 9,
        body,
    })
    .expect("serializes")
}

#[test]
fn events_serialize_with_seq_at_and_tag() {
    assert_eq!(
        value(Body::Input {
            id: "dispatch-01234567".to_owned(),
            text: "- [ ] x".to_owned(),
            by: "github:2".to_owned(),
            login: None,
            origin: Origin::Dispatch,
        }),
        json!({"seq": 3, "at": 9, "t": "input", "id": "dispatch-01234567", "text": "- [ ] x",
               "by": "github:2", "origin": "dispatch"})
    );
    assert_eq!(
        value(Body::AskResolved {
            id: "a".to_owned(),
            outcome: super::Outcome::Withdrawn,
            by: None,
            login: None,
        }),
        json!({"seq": 3, "at": 9, "t": "ask_resolved", "id": "a", "outcome": "withdrawn"})
    );
    assert_eq!(
        value(Body::Usage {
            input: Some(1),
            output: None,
            context: Some(ContextUsage { used: 5, max: 10 }),
        }),
        json!({"seq": 3, "at": 9, "t": "usage", "input": 1, "context": {"used": 5, "max": 10}})
    );
    assert_eq!(
        value(Body::Closed { reason: None }),
        json!({"seq": 3, "at": 9, "t": "closed"})
    );
}

#[test]
fn events_round_trip_through_storage() {
    let event = Event {
        seq: 0,
        at: 1,
        body: Body::Ask {
            id: "req-1".to_owned(),
            kind: AskKind::Tool,
            title: "Bash: ls".to_owned(),
            detail: Some("ls".to_owned()),
            truncated: None,
            options: vec![AskOption {
                id: "allow".to_owned(),
                label: "允许这一次".to_owned(),
                effect: Effect::Allow,
            }],
            questions: None,
        },
    };
    let json = event.to_json().expect("serializes");
    let back: Event = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, event);
}

#[test]
fn fields_are_cut_at_sixteen_kibibytes() {
    let (short, cut) = field("abc");
    assert_eq!((short.as_str(), cut), ("abc", false));
    let (long, cut) = field(&"é".repeat(FIELD_BYTES));
    assert!(cut);
    assert!(long.len() <= FIELD_BYTES);
}

#[test]
fn escaped_tool_fields_are_shrunk_to_the_event_limit() {
    let noisy = "\u{1b}".repeat(FIELD_BYTES);
    let event = fit(Event {
        seq: 0,
        at: 0,
        body: Body::Tool {
            id: "t".to_owned(),
            name: "Bash".to_owned(),
            state: ToolState::Ok,
            kind: Some(ToolKind::Shell),
            title: None,
            input: Some(noisy.clone()),
            output: Some(noisy),
            truncated: None,
        },
    });
    let json = event.to_json().expect("serializes");
    assert!(json.len() <= EVENT_BYTES);
    assert!(matches!(
        event.body,
        Body::Tool {
            truncated: Some(true),
            ..
        }
    ));
}

#[test]
fn events_within_the_limit_are_untouched() {
    let event = Event {
        seq: 0,
        at: 0,
        body: Body::Text {
            mid: "m".to_owned(),
            text: "hello".to_owned(),
        },
    };
    assert_eq!(fit(event.clone()), event);
}

#[test]
fn long_texts_split_into_event_sized_chunks() {
    assert_eq!(chunks(""), vec![""]);
    assert_eq!(chunks("short"), vec!["short"]);
    for text in [
        "a".repeat(TEXT_CHUNK_BYTES * 3),
        "中".repeat(TEXT_CHUNK_BYTES),
        "\u{1}".repeat(TEXT_CHUNK_BYTES),
        "\"\\\n".repeat(TEXT_CHUNK_BYTES / 2),
    ] {
        let parts = chunks(&text);
        assert!(parts.len() > 1);
        assert_eq!(parts.concat(), text);
        for part in parts {
            let json = serde_json::to_string(part).expect("serializes");
            assert!(json.len() <= TEXT_CHUNK_BYTES + 2, "{}", json.len());
        }
    }
}

fn fits(event: &Event) -> bool {
    event.to_json().expect("json").len() <= EVENT_BYTES
}

#[test]
fn every_kind_with_unbounded_text_is_shrunk_to_fit() {
    let huge = "\u{1}".repeat(40_000);
    let bodies = [
        Body::Turn {
            state: super::TurnState::Failed,
            reason: Some(huge.clone()),
        },
        Body::Closed {
            reason: Some(huge.clone()),
        },
        Body::Todo {
            items: (0..10)
                .map(|_| super::TodoItem {
                    text: huge.clone(),
                    state: super::TodoState::Pending,
                })
                .collect(),
        },
        Body::Ask {
            id: "a".to_owned(),
            kind: AskKind::Question,
            title: "t".to_owned(),
            detail: None,
            truncated: None,
            options: Vec::new(),
            questions: Some(vec![super::Question {
                key: "q0".to_owned(),
                header: Some(huge.clone()),
                question: huge.clone(),
                options: vec![super::Choice {
                    label: huge.clone(),
                    description: Some(huge.clone()),
                }],
                multi: false,
                other: false,
            }]),
        },
    ];
    for body in bodies {
        let kind = format!("{body:?}").chars().take(12).collect::<String>();
        let event = fit(Event {
            seq: 1,
            at: 2,
            body,
        });
        assert!(fits(&event), "{kind}");
        assert!(
            !matches!(event.body, Body::Notice { .. }),
            "{kind} kept its kind"
        );
    }
}

#[test]
fn an_event_with_too_many_entries_drops_trailing_ones() {
    let items = (0..20_000)
        .map(|_| super::TodoItem {
            text: String::new(),
            state: super::TodoState::Done,
        })
        .collect();
    let event = fit(Event {
        seq: 1,
        at: 2,
        body: Body::Todo { items },
    });
    assert!(fits(&event));
    assert!(matches!(&event.body, Body::Todo { items } if !items.is_empty()));
}

#[test]
fn an_event_that_cannot_shrink_becomes_an_error_notice_with_its_seq() {
    let event = fit(Event {
        seq: 7,
        at: 2,
        body: Body::Usage {
            input: Some(1),
            output: None,
            context: None,
        },
    });
    assert!(
        matches!(event.body, Body::Usage { .. }),
        "small events are untouched"
    );
    let options = (0..5_000)
        .map(|index| AskOption {
            id: format!("o{index}"),
            label: String::new(),
            effect: Effect::Allow,
        })
        .collect();
    let event = fit(Event {
        seq: 7,
        at: 2,
        body: Body::Ask {
            id: "a".to_owned(),
            kind: AskKind::Tool,
            title: String::new(),
            detail: None,
            truncated: None,
            options,
            questions: None,
        },
    });
    assert_eq!(event.seq, 7);
    assert!(matches!(
        event.body,
        Body::Notice {
            level: super::Level::Error,
            ..
        }
    ));
}
