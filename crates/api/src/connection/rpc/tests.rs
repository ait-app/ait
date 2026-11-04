use serde_json::json;

use super::*;

#[tokio::test]
async fn observed_creation_forwards_progress_and_preserves_its_result() {
    let (source, mut receiver) = Outbound::new();
    let (observer, mut events) = Outbound::new();
    let progress = ServerMessage::Event {
        method: "creation.update".into(),
        params: json!({"phase": "running"}),
    };
    source.send(&progress).unwrap();
    source
        .respond("private".into(), Ok(json!({"agentId": "created"})))
        .unwrap();
    assert_eq!(
        receive(&mut receiver, "private", Some(&observer)).await,
        Ok(json!({"agentId": "created"}))
    );
    let event = events.try_recv().unwrap();
    let Frame::Text(text) = event.message else {
        panic!("expected progress event");
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        serde_json::to_value(progress).unwrap()
    );
    assert!(
        events.try_recv().is_err(),
        "the private response must not escape"
    );
}

#[tokio::test]
async fn disconnected_creation_observers_do_not_drop_the_created_resource_id() {
    let (source, mut receiver) = Outbound::new();
    let (observer, events) = Outbound::new();
    drop(events);
    source
        .send(&ServerMessage::Event {
            method: "creation.update".into(),
            params: json!({}),
        })
        .unwrap();
    source
        .respond("private".into(), Ok(json!({"agentId": "late"})))
        .unwrap();
    assert_eq!(
        receive(&mut receiver, "private", Some(&observer)).await,
        Ok(json!({"agentId": "late"}))
    );
}
