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

fn secret_request() -> Value {
    json!({
        "id": "secret-dialog", "provider": "omp", "name": "extension_ui", "kind": "question",
        "title": "sensitive-title", "description": "sensitive-description",
        "input": {"questions": [
            {"question": "Public", "secret": false},
            {"question": "Private", "secret": true, "initialValue": "sensitive-prefill"}
        ], "answers": {"value": "sensitive-answer"}},
        "detail": {"input": "sensitive-detail"}, "metadata": {"value": "sensitive-metadata"},
        "suggestions": [{"value": "sensitive-suggestion"}],
        "actions": [{"id": "allow", "label": "Submit", "behavior": "allow"}]
    })
}

fn assert_cancel_only(request: &Value) {
    assert_eq!(request["id"], "secret-dialog");
    assert_eq!(request["provider"], "omp");
    assert_eq!(request["kind"], "tool");
    assert!(request["description"].as_str().unwrap().contains("Upgrade"));
    assert_eq!(
        request["actions"],
        json!([{"id": "deny", "label": "Cancel", "behavior": "deny"}])
    );
    for field in ["input", "detail", "metadata", "suggestions"] {
        assert!(request.get(field).is_none(), "unexpected field: {field}");
    }
    assert!(!request.to_string().contains("sensitive-"));
}

#[test]
fn secret_projection_isolates_get_list_and_prepared_snapshot_views() {
    let agent = json!({"id": "one", "pendingPermissions": [secret_request()]});
    let shared = json!({
        "agent": agent, "agents": [agent], "entries": [{"agent": agent}],
        "response": {"entries": [{"agent": agent}]},
        "opaqueProviderData": {"agent": agent}
    });
    let mut legacy = shared.clone();
    crate::dispatch::permission_projection::response(&mut legacy, false);
    for request in [
        &legacy["agent"]["pendingPermissions"][0],
        &legacy["agents"][0]["pendingPermissions"][0],
        &legacy["entries"][0]["agent"]["pendingPermissions"][0],
        &legacy["response"]["entries"][0]["agent"]["pendingPermissions"][0],
    ] {
        assert_cancel_only(request);
    }
    assert_eq!(legacy["opaqueProviderData"], shared["opaqueProviderData"]);
    let mut capable = shared.clone();
    crate::dispatch::permission_projection::response(&mut capable, true);
    assert_eq!(capable, shared);
    assert_eq!(shared["agent"]["pendingPermissions"][0], secret_request());
}

#[test]
fn secret_projection_preserves_nonsecret_questions_and_existing_tools() {
    let mut question = secret_request();
    question["input"]["questions"][1]["secret"] = json!(false);
    let original = json!({"agent": {"pendingPermissions": [
        question, {"id": "tool", "kind": "tool", "input": {"command": "echo hello"}}
    ]}});
    let mut projected = original.clone();
    crate::dispatch::permission_projection::response(&mut projected, false);
    assert_eq!(projected, original);
    for original in [Value::Null, json!({}), json!({"agent": null})] {
        let mut projected = original.clone();
        crate::dispatch::permission_projection::response(&mut projected, false);
        assert_eq!(projected, original);
    }
}

#[test]
fn secret_projection_seeds_pages_and_keeps_connection_specific_updates() {
    let mut first = entry("one", "One");
    first["agent"]["pendingPermissions"] = json!([secret_request()]);
    let mut second = entry("two", "Two");
    second["agent"]["pendingPermissions"] = json!([secret_request()]);
    let shared = prepared(vec![first, second]);
    let mut legacy_prepared = shared.clone();
    crate::dispatch::permission_projection::response(&mut legacy_prepared, false);
    let (page, previous) = unpack(legacy_prepared).unwrap();
    assert_cancel_only(&page["entries"][0]["agent"]["pendingPermissions"][0]);
    assert_cancel_only(&previous["two"]["agent"]["pendingPermissions"][0]);
    let mut legacy = Observation {
        id: "legacy".to_owned(),
        params: json!({}),
        previous,
    };
    let mut capable = Observation {
        id: "capable".to_owned(),
        params: json!({}),
        previous: unpack(shared.clone()).unwrap().1,
    };
    let mut changed = shared;
    changed["entries"][1]["agent"]["pendingPermissions"][0]["input"]["questions"][1]["initialValue"] =
        json!("changed-secret");
    let mut legacy_changed = changed.clone();
    crate::dispatch::permission_projection::response(&mut legacy_changed, false);
    assert_eq!(
        legacy.update(legacy_changed).unwrap(),
        [] as [ServerMessage; 0]
    );
    let events = capable.update(changed.clone()).unwrap();
    let [ServerMessage::Event { params, .. }] = events.as_slice() else {
        panic!("expected capable-only permission update");
    };
    assert_eq!(params["subscriptionId"], "capable");
    assert_eq!(
        params["agent"]["pendingPermissions"][0]["input"]["questions"][1]["initialValue"],
        "changed-secret"
    );
    changed["entries"][1]["agent"]["pendingPermissions"] = json!([]);
    crate::dispatch::permission_projection::response(&mut changed, false);
    let events = legacy.update(changed).unwrap();
    let [ServerMessage::Event { params, .. }] = events.as_slice() else {
        panic!("expected permission removal update");
    };
    assert_eq!(params["subscriptionId"], "legacy");
    assert_eq!(params["agent"]["pendingPermissions"], json!([]));
}

#[test]
fn secret_projection_preserves_sequenced_subscription_progress() {
    let mut row = entry("one", "One");
    row["agent"]["pendingPermissions"] = json!([secret_request()]);
    row["syncSeq"] = json!(7);
    let mut prepared = json!({
        "entries": [row], "response": {"entries": [row], "sync": {
            "generation": "generation", "headSeq": 7, "mode": "changes", "removals": []
        }}
    });
    crate::dispatch::permission_projection::response(&mut prepared, false);
    let mut observation = observation(Vec::new());
    let events = observation.update(prepared).unwrap();
    let [ServerMessage::Event { params, .. }] = events.as_slice() else {
        panic!("expected sequenced update");
    };
    assert_cancel_only(&params["agent"]["pendingPermissions"][0]);
    assert_eq!(params["seq"], 7);
    assert_eq!(observation.params["sync"]["afterSeq"], 7);
}
