use super::*;

fn entry(id: &str, title: &str) -> Value {
    json!({"agent":{"id":id,"title":title},"project":{"projectKey":"project"}})
}

fn prepared(entries: Vec<Value>) -> Value {
    let response = json!({"entries":entries.iter().take(1).collect::<Vec<_>>()});
    let mut value = json!({"response":response});
    value["entries"] = Value::Array(entries);
    value
}

fn observation(entries: Vec<Value>) -> Observation {
    Observation {
        id: "owned".to_owned(),
        params: json!({}),
        previous: unpack(prepared(entries)).unwrap().1,
    }
}

#[test]
fn pages_seed_the_entire_filter_without_emitting_unseen_page_rows() {
    let mut observation = observation(vec![entry("one", "One"), entry("two", "Two")]);
    assert_eq!(
        observation
            .update(prepared(vec![entry("one", "One"), entry("two", "Two")]))
            .unwrap(),
        [] as [ServerMessage; 0]
    );
    let events = observation
        .update(prepared(vec![
            entry("two", "Changed"),
            entry("three", "Three"),
        ]))
        .unwrap();
    assert_eq!(events.len(), 3);
    let params: Vec<_> = events
        .iter()
        .map(|event| match event {
            ServerMessage::Event { params, .. } => params,
            _ => panic!("expected event"),
        })
        .collect();
    assert!(
        params
            .iter()
            .all(|params| params["subscriptionId"] == "owned")
    );
    assert!(
        params
            .iter()
            .any(|params| params["agent"]["id"] == "two" && params["agent"]["title"] == "Changed")
    );
    assert!(
        params
            .iter()
            .any(|params| params["kind"] == "remove" && params["agentId"] == "one")
    );
}

#[test]
fn sequenced_events_keep_global_sequences_and_advance_the_observer_checkpoint() {
    let mut observation = observation(vec![entry("one", "One")]);
    observation.params["sync"] = json!({"generation":"generation","afterSeq":1});
    let mut two = entry("two", "Two");
    two["syncSeq"] = json!(3);
    let events = observation.update(json!({"entries":[entry("two","Two")],"response":{
        "entries":[two],"sync":{"generation":"generation","headSeq":3,"mode":"changes","removals":[{"id":"one","seq":2}]}
    }})).unwrap();
    assert_eq!(events.len(), 2);
    for event in &events {
        let ServerMessage::Event { params, .. } = event else {
            panic!("expected event")
        };
        assert_eq!(params["generation"], "generation");
        assert!(params.get("seq").is_some());
        assert!(params.get("syncSeq").is_none());
    }
    assert_eq!(observation.params["sync"]["afterSeq"], 3);
}

#[test]
fn invalid_poll_response_does_not_advance_the_checkpoint() {
    let mut observation = observation(vec![entry("one", "One")]);
    assert!(observation.update(json!({"entries":[]})).is_err());
    assert!(observation.update(json!({"entries":[{}]})).is_err());
    let result = observation.update(json!({"entries":[],"response":{"sync":{"mode":"snapshot"}}}));
    assert_eq!(result.unwrap_err(), ErrorCode::AgentIo);
}
