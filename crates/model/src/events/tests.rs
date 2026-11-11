use super::*;
use crate::outbound::Frame;
use serde_json::json;

#[derive(Debug, Default)]
struct Publisher(Mutex<Vec<(String, Value)>>);

impl crate::hub::EventPublisher for Publisher {
    fn publish(&self, method: &str, params: &Value) -> bool {
        self.0
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        false
    }
}

#[test]
fn publisher_gets_one_unfiltered_commit_independently_of_client_observers() {
    let hub = EventHub::default();
    let publisher = Arc::new(Publisher::default());
    hub.set_publisher(publisher.clone());
    let (outbound, mut receiver) = Outbound::new();
    let subscription = hub.subscribe(
        "sub".to_owned(),
        BTreeSet::from(["other".to_owned()]),
        outbound,
        false,
    );
    subscription.activate().unwrap();
    hub.publish("agent", "agent_stream", &json!({"seq":1}));
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        publisher.0.lock().unwrap().as_slice(),
        &[("agent_stream".to_owned(), json!({"seq":1}))]
    );
    hub.publish("agent", "agent_stream", &Value::Null);
    assert_eq!(publisher.0.lock().unwrap().len(), 1);
}

#[test]
fn operation_observers_emit_progress_without_connection_subscription_identity() {
    let hub = EventHub::default();
    let (outbound, mut receiver) = Outbound::new();
    let operation = hub.observe("operation".into(), BTreeSet::from(["key".into()]), outbound);
    hub.publish("key", "progress", &json!({"phase":"accepted"}));
    operation.activate().unwrap();
    assert_eq!(next(&mut receiver)["params"], json!({"phase":"accepted"}));
    drop(operation);
    hub.publish("key", "progress", &json!({"phase":"completed"}));
    assert!(receiver.try_recv().is_err());
}

fn next(receiver: &mut tokio::sync::mpsc::Receiver<crate::outbound::Queued>) -> Value {
    let message = receiver.try_recv().unwrap();
    let Frame::Text(text) = message.message else {
        panic!("expected JSON");
    };
    serde_json::from_str(&text).unwrap()
}

#[test]
fn observers_start_paused_preserve_order_and_stop_on_drop() {
    let hub = EventHub::default();
    let (outbound, mut receiver) = Outbound::new();
    let subscription = hub.subscribe(
        "sub".to_owned(),
        BTreeSet::from(["agent".to_owned()]),
        outbound.clone(),
        false,
    );
    hub.publish("other", "update", &json!({"revision":0}));
    hub.publish("agent", "update", &json!({"revision":1}));
    assert!(receiver.try_recv().is_err());
    outbound
        .respond(
            "request".to_owned(),
            Ok(json!({"subscriptionId":subscription.id()})),
        )
        .unwrap();
    subscription.activate().unwrap();
    assert_eq!(next(&mut receiver)["type"], "response");
    assert_eq!(
        next(&mut receiver)["params"],
        json!({"revision":1,"subscriptionId":"sub"})
    );
    hub.publish("agent", "update", &json!({"revision":2}));
    assert_eq!(next(&mut receiver)["params"]["revision"], 2);
    drop(subscription);
    hub.publish("agent", "update", &json!({"revision":3}));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn pending_overflow_and_closed_transport_cancel_delivery() {
    let hub = EventHub::default();
    let (outbound, receiver) = Outbound::new();
    let subscription = hub.subscribe(
        "sub".to_owned(),
        BTreeSet::from(["agent".to_owned()]),
        outbound.clone(),
        false,
    );
    for revision in 0..65 {
        hub.publish("agent", "update", &json!({"revision":revision}));
    }
    assert!(outbound.failure().is_cancelled());
    assert!(subscription.activate().is_err());
    drop(receiver);
    let (outbound, receiver) = Outbound::new();
    let subscription = hub.subscribe(
        "second".to_owned(),
        BTreeSet::from(["agent".to_owned()]),
        outbound.clone(),
        false,
    );
    subscription.activate().unwrap();
    drop(receiver);
    hub.publish("agent", "update", &json!({"revision":99}));
    assert!(outbound.failure().is_cancelled());
}

#[test]
fn legacy_timeline_permission_events_remove_secret_fields_before_queueing() {
    let hub = EventHub::default();
    let (outbound, mut receiver) = Outbound::new();
    let subscription = hub.subscribe(
        "legacy".into(),
        BTreeSet::from(["agent".into()]),
        outbound,
        false,
    );
    let (capable_outbound, mut capable_receiver) = Outbound::new();
    let capable = hub.subscribe(
        "capable".into(),
        BTreeSet::from(["agent".into()]),
        capable_outbound,
        true,
    );
    let request = json!({
        "id":"question", "provider":"omp", "kind":"question",
        "input":{"questions":[{"secret":true,"initialValue":"private-prefill"}]},
        "detail":{"input":"private-detail"},
        "actions":[{"id":"allow","behavior":"allow"}]
    });
    hub.publish(
        "agent",
        "agent_stream",
        &json!({
            "agentId":"agent",
            "event":{"type":"permission_requested","request":request},
            "timestamp":"2026-11-11T00:00:00Z"
        }),
    );
    subscription.activate().unwrap();
    capable.activate().unwrap();
    assert_eq!(
        next(&mut capable_receiver)["params"]["event"]["request"],
        request
    );
    let delivered = next(&mut receiver);
    let card = &delivered["params"]["event"]["request"];
    assert_eq!(card["kind"], "tool");
    assert_eq!(
        card["actions"],
        json!([{"id":"deny","label":"Cancel","behavior":"deny"}])
    );
    assert!(card.get("input").is_none());
    assert!(card.get("detail").is_none());
    assert_eq!(
        request["input"]["questions"][0]["initialValue"],
        "private-prefill"
    );
}
