//! Read-only preview behavior: fixed cuts, identity binding, pagination and writer immutability.

use serde_json::{Value, json};

use super::fixture::Fixture;
use crate::ports::agent_session::AgentClient;
use crate::ports::native_history::PreviewRequest;

fn request(fixture: &Fixture, limit: usize) -> PreviewRequest {
    PreviewRequest {
        provider: "deepseek-harness".into(),
        session_id: "session".into(),
        cwd: fixture.spec.cwd.clone(),
        cursor: None,
        limit,
    }
}

/// Twelve records form six user/assistant turns, enough to force several wire pages.
fn turn_records(turns: usize) -> Vec<Value> {
    let mut records = Vec::new();
    for turn in 0..turns {
        for (kind, data) in [
            (
                "user/message",
                json!({"id":format!("user-{turn}"),"source":{"kind":"user"},
                    "content":[{"type":"text","text":format!("question {turn}")}]}),
            ),
            ("turn/start", json!({"turn":turn})),
            (
                "assistant/message",
                json!({"turn":turn,"message":{"id":format!("answer-{turn}"),
                    "content":[{"type":"text","text":format!("answer {turn}")}]}}),
            ),
            (
                "turn/end",
                json!({"turn":turn,"reason":{"kind":"completed"}}),
            ),
        ] {
            let seq = records.len();
            records.push(json!({"type":"event","event":{
                "seq":seq,"type":kind,"time":1_700_000_000_000_u64+seq as u64,"data":data}}));
        }
    }
    records
}

#[tokio::test]
async fn preview_pages_newest_first_on_a_fixed_cut_without_writer_requests() {
    let fixture = Fixture::new().await;
    fixture.seed_records(turn_records(6));
    let first = fixture
        .client
        .preview_session(&request(&fixture, 4))
        .await
        .unwrap();
    assert_eq!(first.entries.len(), 4);
    assert!(!first.active);
    assert_eq!(first.descriptor.provider_handle_id, "session");
    assert_eq!(first.descriptor.cwd, fixture.spec.cwd);
    let texts: Vec<_> = first
        .entries
        .iter()
        .map(|entry| entry.item["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        texts,
        ["answer 5", "question 5", "answer 4", "question 4"],
        "the newest page shows the latest turns first"
    );
    let cursor = first.older_cursor.expect("older history remains");
    assert_eq!(cursor.through_seq, 23);
    assert_eq!(cursor.provider_handle_id, "session");
    assert_eq!(cursor.cwd, fixture.spec.cwd);

    let mut older_request = request(&fixture, 100);
    older_request.cursor = Some(cursor);
    let second = fixture
        .client
        .preview_session(&older_request)
        .await
        .unwrap();
    assert_eq!(second.entries.len(), 8);
    assert!(second.older_cursor.is_none(), "the cut is exhausted");
    let older_texts: Vec<_> = second
        .entries
        .iter()
        .map(|entry| entry.item["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        older_texts,
        [
            "answer 3",
            "question 3",
            "answer 2",
            "question 2",
            "answer 1",
            "question 1",
            "answer 0",
            "question 0"
        ]
    );
    for method in [
        "session/follow",
        "session/create",
        "session/prompt",
        "session/selectModel",
        "commands/execute",
    ] {
        assert!(
            fixture.requests(method).is_empty(),
            "{method} must never run while previewing"
        );
    }
    let pages = fixture.requests("session/page");
    assert!(
        pages.len() > 1,
        "older pages must be read through session/page"
    );
    assert!(
        pages.iter().all(|page| page["request"]["throughSeq"] == 23),
        "every page keeps the fixed native cut"
    );
    assert!(
        pages
            .iter()
            .all(|page| page["request"]["address"]["sessionId"] == "session"),
        "every page stays bound to the validated native identity"
    );
}

#[tokio::test]
async fn preview_keeps_reading_the_original_cut_after_native_history_grows() {
    let fixture = Fixture::new().await;
    fixture.seed_records(turn_records(2));
    let first = fixture
        .client
        .preview_session(&request(&fixture, 2))
        .await
        .unwrap();
    let cursor = first.older_cursor.expect("older history remains");
    assert_eq!(cursor.through_seq, 7);
    // The native log grows after the preview opened; the continuation must not drift.
    fixture.seed_records(turn_records(6));
    let mut older_request = request(&fixture, 100);
    older_request.cursor = Some(cursor);
    let second = fixture
        .client
        .preview_session(&older_request)
        .await
        .unwrap();
    let texts: Vec<_> = second
        .entries
        .iter()
        .map(|entry| entry.item["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(texts, ["answer 0", "question 0"]);
    assert!(
        fixture
            .requests("session/page")
            .iter()
            .all(|page| page["request"]["throughSeq"] == 7)
    );
    assert!(fixture.requests("session/follow").is_empty());
}

#[tokio::test]
async fn preview_rejects_foreign_stale_and_forged_cursors_before_native_reads() {
    let fixture = Fixture::new().await;
    fixture.seed_records(turn_records(2));
    let first = fixture
        .client
        .preview_session(&request(&fixture, 2))
        .await
        .unwrap();
    let cursor = first.older_cursor.expect("older history remains");
    let reads_before = fixture.requests("session/page").len();
    let forgeries = [
        PreviewRequest {
            session_id: "other-session".into(),
            cursor: Some(cursor.clone()),
            ..request(&fixture, 10)
        },
        PreviewRequest {
            cwd: "/elsewhere".into(),
            cursor: Some(cursor.clone()),
            ..request(&fixture, 10)
        },
        PreviewRequest {
            cursor: Some(crate::ports::native_history::PreviewCursor {
                before_seq: cursor.through_seq + 2,
                ..cursor.clone()
            }),
            ..request(&fixture, 10)
        },
        PreviewRequest {
            cursor: Some(crate::ports::native_history::PreviewCursor {
                through_seq: -2,
                ..cursor.clone()
            }),
            ..request(&fixture, 10)
        },
    ];
    for forged in forgeries {
        assert!(
            fixture.client.preview_session(&forged).await.is_err(),
            "cross-scope or stale cursors must be rejected"
        );
    }
    assert_eq!(
        fixture.requests("session/page").len(),
        reads_before,
        "rejected cursors must not reach the native Host"
    );
    assert!(fixture.requests("session/follow").is_empty());
    assert!(fixture.requests("session/create").is_empty());
}

#[tokio::test]
async fn preview_rejects_foreign_or_subagent_identities_without_writes() {
    let fixture = Fixture::new().await;
    fixture.seed_records(turn_records(1));
    for summary in [
        json!({"sessionId":"different","cwd":fixture.spec.cwd}),
        json!({"sessionId":"session","cwd":"/elsewhere"}),
        json!({"sessionId":"session","cwd":fixture.spec.cwd,"origin":"subagent"}),
    ] {
        fixture.set_sessions(json!([summary]));
        assert!(
            fixture
                .client
                .preview_session(&request(&fixture, 10))
                .await
                .is_err()
        );
    }
    for method in [
        "session/follow",
        "session/create",
        "session/prompt",
        "session/selectModel",
        "session/page",
    ] {
        assert!(
            fixture.requests(method).is_empty(),
            "{method} must not run for a rejected identity"
        );
    }
}

#[tokio::test]
async fn preview_invalid_baselines_fail_without_activating_fallbacks() {
    for baseline in [
        Value::Null,
        json!({"kind":"migration-required","values":{}}),
        json!({"kind":"sequenced","asOfSeq":-2,"values":{}}),
        json!({"kind":"sequenced","asOfSeq":i64::MAX,"values":{}}),
        json!({"kind":"sequenced","asOfSeq":0,"values":null}),
    ] {
        let fixture = Fixture::new().await;
        fixture.reply("session/projections", baseline);
        assert!(
            fixture
                .client
                .preview_session(&request(&fixture, 10))
                .await
                .is_err()
        );
        assert!(fixture.requests("session/follow").is_empty());
        assert!(fixture.requests("session/create").is_empty());
    }
    for scenario in ["404", "500", "rejected"] {
        let fixture = Fixture::new().await;
        fixture.fail("session/projections", scenario);
        assert!(
            fixture
                .client
                .preview_session(&request(&fixture, 10))
                .await
                .is_err()
        );
        assert!(fixture.requests("session/follow").is_empty());
    }
}

#[tokio::test]
async fn preview_empty_history_keeps_the_native_negative_cursor() {
    let fixture = Fixture::new().await;
    fixture.seed_records(Vec::new());
    let preview = fixture
        .client
        .preview_session(&request(&fixture, 10))
        .await
        .unwrap();
    assert!(preview.entries.is_empty());
    assert!(!preview.active);
    assert!(preview.older_cursor.is_none());
    assert_eq!(
        fixture.requests("session/page")[0]["request"]["throughSeq"],
        -1
    );
    assert!(fixture.requests("session/follow").is_empty());
}

#[tokio::test]
async fn preview_observes_an_unfinished_turn_without_touching_the_writer() {
    let fixture = Fixture::new().await;
    let mut writer = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = writer.persistence().unwrap();
    let mut records = turn_records(1);
    records.pop(); // leave the turn open
    fixture.seed_records(records);
    let preview_request = PreviewRequest {
        session_id: handle.session_id.clone(),
        ..request(&fixture, 10)
    };
    let preview = fixture
        .client
        .preview_session(&preview_request)
        .await
        .unwrap();
    assert!(preview.active, "an unfinished turn is observable");
    assert_eq!(fixture.requests("session/follow").len(), 1);
    assert_eq!(fixture.requests("session/create").len(), 1);
    assert!(fixture.requests("session/prompt").is_empty());
    assert!(fixture.requests("session/selectModel").is_empty());
    // The existing writer keeps its identity and can still complete its turn.
    writer
        .start_turn("continue", &fixture.spec.config)
        .await
        .unwrap();
    assert_eq!(fixture.requests("session/prompt").len(), 1);
    writer.close().await.unwrap();
}

#[tokio::test]
async fn preview_rejects_invalid_limits_and_acp_transport() {
    let fixture = Fixture::new().await;
    for limit in [0, 101] {
        assert!(
            fixture
                .client
                .preview_session(&request(&fixture, limit))
                .await
                .is_err()
        );
    }
    let acp = fixture.client.clone().with_acp_profile();
    assert!(!acp.supports_session_preview());
    assert!(acp.preview_session(&request(&fixture, 10)).await.is_err());
    assert!(fixture.requests("session/page").is_empty());
    assert!(fixture.requests("session/follow").is_empty());
}
