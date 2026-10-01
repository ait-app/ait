use super::*;
use serde_json::json;

mod physical;

fn message(length: usize) -> ServerMessage {
    ServerMessage::Response {
        request_id: "1".to_owned(),
        result: json!("x".repeat(length)),
    }
}

#[test]
fn count_budget_rejects_slow_consumer_and_releases_on_drop() {
    let (queue, mut receiver) = Outbound::new();
    for _ in 0..MAX_QUEUE_MESSAGES {
        queue.send(&message(1)).unwrap();
    }
    assert!(matches!(queue.send(&message(1)), Err(QueueError::Full)));
    assert!(queue.failed.is_cancelled());
    drop(receiver.try_recv().unwrap());
    queue.send(&message(1)).unwrap();
    drop(receiver);
    assert_eq!(queue.bytes.available_permits(), MAX_QUEUE_BYTES);
    assert!(queue.send(&message(1)).is_err());
}

#[test]
fn byte_budget_includes_in_flight_write_and_returns_its_permits() {
    let (queue, mut receiver) = Outbound::new();
    let large = message(MAX_QUEUE_BYTES / 2);
    queue.send(&large).unwrap();
    let in_flight = receiver.try_recv().unwrap();
    assert!(matches!(queue.send(&large), Err(QueueError::Full)));
    drop(in_flight);
    queue.send(&large).unwrap();
    assert!(queue.send(&message(MAX_QUEUE_BYTES)).is_err());
}

#[tokio::test]
async fn fair_lanes_share_the_physical_budget_and_round_robin() {
    let (lanes, mut receiver) = Outbound::fair();
    for index in 0..4 {
        lanes[2]
            .respond(format!("file-{index}"), Ok(serde_json::json!({})))
            .unwrap();
    }
    lanes[1]
        .respond("terminal".to_owned(), Ok(serde_json::json!({})))
        .unwrap();
    lanes[0]
        .respond("ping".to_owned(), Ok(serde_json::json!({})))
        .unwrap();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let queued = receiver.recv().await.unwrap();
        let Frame::Text(text) = &queued.message else {
            panic!("expected JSON")
        };
        ids.push(serde_json::from_str::<serde_json::Value>(text).unwrap()["request_id"].clone());
    }
    assert_eq!(
        ids,
        vec![
            serde_json::json!("ping"),
            serde_json::json!("terminal"),
            serde_json::json!("file-0")
        ]
    );
    while receiver.try_recv().is_ok() {}
    let held = lanes[0]
        .bytes
        .clone()
        .acquire_many_owned(u32::try_from(MAX_QUEUE_BYTES).unwrap())
        .await
        .unwrap();
    assert!(
        lanes[3]
            .respond("over-budget".to_owned(), Ok(serde_json::json!({})))
            .is_err()
    );
    assert!(lanes[1].failure().is_cancelled());
    drop(held);
}
