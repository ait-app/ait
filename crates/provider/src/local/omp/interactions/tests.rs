use super::*;

#[test]
fn every_interactive_method_matches_the_shared_client_contract_and_native_reply() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("../../../../tests/fixtures/omp_dialogs.json")).unwrap();
    for fixture in fixtures {
        let mut dialogs = Dialogs::default();
        assert_eq!(
            dialogs.capture(&fixture["native"]).unwrap(),
            vec![AgentTurnEvent::PermissionRequested(
                fixture["request"].clone()
            )],
            "{}",
            fixture["name"]
        );
        let id = fixture["native"]["id"].as_str().unwrap();
        assert_eq!(dialogs.pending(), vec![fixture["request"].clone()]);
        assert_eq!(
            dialogs.response(id, &fixture["response"]).unwrap(),
            fixture["nativeResponse"]
        );
        assert_eq!(
            dialogs.response(id, &json!({"behavior":"deny"})).unwrap(),
            json!({"type":"extension_ui_response","id":id,"cancelled":true})
        );
        assert_eq!(
            dialogs
                .capture(&json!({"method":"cancel","targetId":id}))
                .unwrap(),
            vec![AgentTurnEvent::PermissionResolved(id.to_owned())]
        );
        assert_eq!(dialogs.pending(), Vec::<Value>::new());
        assert!(dialogs.response(id, &fixture["response"]).is_err());
    }
}

#[test]
fn structured_question_matches_the_client_wire_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/omp_question.json")).unwrap();
    let mut dialogs = Dialogs::default();

    let events = dialogs.capture(&fixture["native"]).unwrap();

    assert_eq!(
        events,
        vec![AgentTurnEvent::PermissionRequested(
            fixture["request"].clone()
        )]
    );
    assert_eq!(dialogs.pending(), vec![fixture["request"].clone()]);
    assert_eq!(
        dialogs.response("ask-1", &fixture["response"]).unwrap(),
        fixture["nativeResponse"]
    );
}

#[test]
fn optional_dialog_descriptions_are_strings_or_absent() {
    for method in ["select", "confirm", "input", "editor", "ask"] {
        for message in [
            None,
            Some(Value::Null),
            Some(json!(42)),
            Some(json!("Details")),
        ] {
            let mut dialogs = Dialogs::default();
            let mut native = json!({"id":"dialog","method":method,"title":"Question",
                "options":["A"],"questions":[{"id":"choice","question":"Choose",
                    "options":[{"label":"A"}]}]});
            if let Some(message) = &message {
                native["message"] = message.clone();
            }

            let events = dialogs.capture(&native).unwrap();

            let [AgentTurnEvent::PermissionRequested(request)] = events.as_slice() else {
                panic!("missing {method} request");
            };
            assert_eq!(
                request.get("description"),
                message.as_ref().filter(|v| v.is_string())
            );
        }
    }
}

#[test]
fn structured_questions_preserve_multiselect_and_commas() {
    let mut dialogs = Dialogs::default();
    let native = json!({"id":"ask","method":"ask","questions":[
        {"id":"db","question":"Database?","options":[{"label":"A,B"}]},
        {"id":"features","question":"Features?","multi":true,"options":[{"label":"Auth"},{"label":"Search"}]}]});
    dialogs.capture(&native).unwrap();
    let response = dialogs
        .response(
            "ask",
            &json!({"behavior":"allow","updatedInput":{"answers":{
        "db":["A,B"],"features":["Auth","Search"]}}}),
        )
        .unwrap();
    assert_eq!(response["answers"][0]["selectedOptions"], json!(["A,B"]));
    assert_eq!(
        response["answers"][1]["selectedOptions"],
        json!(["Auth", "Search"])
    );
    assert!(
        dialogs
            .response(
                "ask",
                &json!({"behavior":"allow","updatedInput":{"answers":{
        "db":["A","B"],"features":[]}}})
            )
            .is_err()
    );
    assert!(
        dialogs
            .response(
                "ask",
                &json!({"behavior":"allow","updatedInput":{"answers":{
        "db":["A,B"],"features":["Auth","Auth"]}}})
            )
            .is_err()
    );
}

#[test]
fn confirmation_cancellation_and_stale_answers_preserve_native_authority() {
    let mut dialogs = Dialogs::default();
    let request = json!({"type":"extension_ui_request","id":"confirm","method":"confirm","title":"Continue?","message":"Run tool"});
    assert_eq!(dialogs.capture(&request).unwrap().len(), 1);
    assert_eq!(
        dialogs
            .response("confirm", &json!({"behavior":"allow"}))
            .unwrap()["confirmed"],
        true
    );
    assert!(
        dialogs
            .response("confirm", &json!({"behavior":"allow","updatedInput":{}}))
            .is_err()
    );
    assert_eq!(
        dialogs
            .response("confirm", &json!({"behavior":"deny"}))
            .unwrap()["cancelled"],
        true
    );
    assert_eq!(
        dialogs
            .capture(&json!({"method":"cancel","targetId":"confirm"}))
            .unwrap()
            .len(),
        1
    );
    assert!(
        dialogs
            .response("confirm", &json!({"behavior":"allow"}))
            .is_err()
    );
    assert_eq!(
        dialogs
            .capture(&json!({"method":"notify","id":"notice"}))
            .unwrap(),
        Vec::new()
    );
}

#[test]
fn selects_and_timeouts_reject_invalid_answers() {
    let mut dialogs = Dialogs::default();
    dialogs
        .capture(&json!({"id":"select","method":"select","title":"Choose","options":["A","B"]}))
        .unwrap();
    assert!(
        dialogs
            .response(
                "select",
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":"C"}}})
            )
            .is_err()
    );
    assert_eq!(
        dialogs
            .response(
                "select",
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":"A"}}})
            )
            .unwrap()["value"],
        "A"
    );
    dialogs
        .capture(&json!({"id":"expired","method":"input","title":"Name","timeout":0}))
        .unwrap();
    assert!(
        dialogs
            .response("expired", &json!({"behavior":"allow"}))
            .is_err()
    );
    assert_eq!(dialogs.pending().len(), 1);
    assert_eq!(dialogs.clear().len(), 2);
}
