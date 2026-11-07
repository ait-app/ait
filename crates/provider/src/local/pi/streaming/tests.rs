use super::*;

#[test]
fn submitted_batch_preserves_cleanup_capacity_and_rejects_atomically() {
    for admission in [false, true] {
        let mut stream = Stream::default();
        stream.begin("turn".into());
        stream.admission = admission;
        let limit = stream.event_limit();
        for _ in 0..limit - 3 {
            stream.push(AgentTurnEvent::Cancelled).unwrap();
        }
        let batch = Stream::user_entries("turn", None, &"x".repeat(MAX_TEXT + 1), &[]);
        assert_eq!(
            stream.insert_submitted(batch),
            Err(AgentSessionError::Failed)
        );
        assert_eq!(stream.events.len(), limit - 3);
        assert_eq!(stream.input_position, 0);
        stream
            .insert_submitted(Stream::user_entries("turn", None, "input", &[]))
            .unwrap();
        assert_eq!(stream.events.len() + stream.tools.len() + 2, limit);
        assert!(matches!(
            stream.events.front(),
            Some(AgentTurnEvent::Timeline(_))
        ));
        assert_eq!(stream.input_position, 1);
    }
}

#[test]
fn submitted_input_reclaims_notice_capacity() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":"notice"}))
        .unwrap();
    assert_eq!(stream.queued_notices.len(), 1);
    while stream.events.len() + 2 < stream.event_limit() {
        stream.push(AgentTurnEvent::Cancelled).unwrap();
    }
    stream
        .insert_submitted(Stream::user_entries("turn", None, "input", &[]))
        .unwrap();
    assert!(stream.queued_notices.is_empty());
    assert_eq!(stream.events.len() + 2, stream.event_limit());
    assert!(matches!(
        stream.events.front(),
        Some(AgentTurnEvent::Timeline(_))
    ));
}
