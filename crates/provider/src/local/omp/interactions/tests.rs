use super::*;

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
