use super::*;
use provider_core::protocol::side_questions::{BtwHistoryTurn, SideTurnStatus};

fn topic() -> SideTopic {
    serde_json::from_value(serde_json::json!({
        "id":"topic-1", "leafId":"native-leaf", "question":"root", "answer":"partial",
        "status":"running", "createdAt":1, "updatedAt":2,
        "strategy":"native", "truncated":false
    }))
    .unwrap()
}

#[test]
fn terminal_record_cannot_be_resurrected_but_a_new_followup_can_start() {
    let mut previous = topic();
    previous.record.status = SideTurnStatus::Complete;
    let mut stale = previous.clone();
    stale.record.status = SideTurnStatus::Running;
    stale.record.updated_at = 99;
    assert!(!advances(&previous, &stale));
    stale.record.follow_ups.push(BtwHistoryTurn {
        question: "followup".to_owned(),
        images: Vec::new(),
        answer: String::new(),
        status: SideTurnStatus::Running,
        created_at: 100,
        updated_at: 100,
        error: None,
    });
    assert!(advances(&previous, &stale));
    assert!(!advances(&stale, &previous));
}

#[test]
fn out_of_order_stream_frames_cannot_shrink_or_rewind_the_answer() {
    let previous = topic();
    let mut incoming = previous.clone();
    incoming.record.answer = "p".to_owned();
    incoming.record.updated_at = 3;
    assert!(!advances(&previous, &incoming));
    incoming.record.answer = "partial new".to_owned();
    incoming.record.updated_at = 1;
    assert!(!advances(&previous, &incoming));
    incoming.record.updated_at = 3;
    assert!(advances(&previous, &incoming));
    incoming.record.status = SideTurnStatus::Cancelled;
    incoming.record.answer.clear();
    assert!(advances(&previous, &incoming));
}

#[test]
fn escaped_multiturn_history_is_bounded_without_losing_latest_status_or_native_identity() {
    let mut topic = topic();
    topic.record.answer = "\"\\世界".repeat(12000);
    for index in 0..7 {
        topic.record.follow_ups.push(BtwHistoryTurn {
            question: "q".repeat(5000),
            images: Vec::new(),
            answer: "\"\\世界".repeat(12000),
            status: SideTurnStatus::Running,
            created_at: index + 3,
            updated_at: index + 3,
            error: None,
        });
    }
    bound(&mut topic).unwrap();
    assert!(topic.truncated);
    assert!(serde_json::to_vec(&topic).unwrap().len() <= MAX_RECORD_BYTES);
    assert_eq!(topic.record.id, "topic-1");
    assert_eq!(topic.record.leaf_id.as_deref(), Some("native-leaf"));
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Running);
    assert!(!topic.latest_turn().answer.is_empty());
    assert!(
        topic
            .record
            .follow_ups
            .iter()
            .all(|turn| turn.question.len() <= MAX_QUESTION_BYTES)
    );
}

#[test]
fn native_error_credentials_are_redacted_before_projection_storage() {
    let mut topic = topic();
    topic.record.error = Some(
        "failed api_key=fixture-secret https://user:pass@example.test/private?token=hidden"
            .to_owned(),
    );
    bound(&mut topic).unwrap();
    let error = topic.record.error.unwrap();
    assert!(!error.contains("fixture-secret"));
    assert!(!error.contains("user:pass"));
    assert!(!error.contains("hidden"));
    assert!(error.contains("failed"));
}
