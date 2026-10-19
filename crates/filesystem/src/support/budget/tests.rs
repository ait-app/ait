use model::ServerMessage;
use model::outbound::Outbound;
use serde_json::{Value, json};

use super::*;

fn largest_budgeted_response(request_id: &str) -> ServerMessage {
    let envelope = serde_json::to_string(&ServerMessage::Response {
        request_id: request_id.to_owned(),
        result: json!(""),
    })
    .unwrap()
    .len();
    let result = Value::String("x".repeat(OUTPUT_BUDGET - 2));
    assert!(fits(&result));
    assert!(!fits(&Value::String("x".repeat(OUTPUT_BUDGET - 1))));
    let message = ServerMessage::Response {
        request_id: request_id.to_owned(),
        result,
    };
    assert_eq!(
        serde_json::to_string(&message).unwrap().len(),
        envelope + OUTPUT_BUDGET - 2
    );
    message
}

#[tokio::test]
async fn budgeted_response_leaves_room_for_every_other_lane() {
    let (lanes, _receiver) = Outbound::fair();
    // A diff waits for output credit and keeps it until its write completes.
    lanes[2]
        .send_waiting(&largest_budgeted_response("diff"))
        .await
        .unwrap();
    // Responses on the other lanes reserve credit without waiting; a miss fails the connection.
    for (lane, request_id) in [(0, "metadata"), (1, "terminal"), (3, "provider")] {
        lanes[lane]
            .send(&largest_budgeted_response(request_id))
            .unwrap();
    }
    assert!(!lanes[0].failure().is_cancelled());
}
