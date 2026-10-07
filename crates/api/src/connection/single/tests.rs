use super::*;

#[test]
fn worker_lanes_keep_streams_and_subscriptions_with_their_connection_owner() {
    for (method, expected) in [
        ("terminal.input", 1),
        ("terminal.list.request", 1),
        ("voice.abort.request", 1),
        ("dictation.stream.start", 1),
        ("checkout.diff.subscribe.request", 2),
        ("file.upload.request", 2),
        ("agent.list.request", 3),
        ("agent.timeline.set_subscription.request", 3),
        ("workspace.create.request", 0),
        ("schedule.list.request", 0),
        ("browser.host.register.request", 0),
        ("relay.status.request", 0),
        ("unknown.request", 0),
    ] {
        let message = Incoming::Text(
            ClientMessage::Request {
                request_id: "r1".to_owned(),
                method: method.to_owned(),
                params: serde_json::json!({}),
            },
            None,
        );
        assert_eq!(lane(&message), expected, "{method}");
    }
    assert_eq!(lane(&Incoming::Binary(vec![0x01])), 1);
    assert_eq!(lane(&Incoming::Binary(vec![0x10])), 2);
    assert_eq!(lane(&Incoming::Binary(Vec::new())), 2);
}
