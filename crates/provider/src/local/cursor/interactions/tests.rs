use super::*;

fn question() -> Value {
    json!({"id":42,"method":"cursor/ask_question","params":{"toolCallId":"question",
        "questions":[{"id":"q1","prompt":"Choose","allowMultiple":true,"options":[
            {"id":"a","label":"Alpha"},{"id":"b","label":"Beta"}]}]}})
}

#[test]
fn maps_labels_to_native_option_ids_and_validates_answers() {
    let pending = Pending::capture(&question()).unwrap();
    let response = pending
        .resolve(&json!({"behavior":"allow","updatedInput":{"answers":{"q1":["Alpha","Beta"]}}}))
        .unwrap();
    assert_eq!(
        response["result"]["outcome"]["answers"][0]["selectedOptionIds"],
        json!(["a", "b"])
    );
    assert_eq!(
        pending.resolve(&json!({"behavior":"deny"})).unwrap()["result"]["outcome"]["outcome"],
        "skipped"
    );
    for answers in [
        json!({}),
        json!({"q1":["Alpha","Alpha"]}),
        json!({"q1":["Other"]}),
        json!({"q1":[]}),
    ] {
        assert!(
            pending
                .resolve(&json!({"behavior":"allow","updatedInput":{"answers":answers}}))
                .is_err()
        );
    }
    let mut single = question();
    single["params"]["questions"][0]["allowMultiple"] = json!(false);
    assert!(
        Pending::capture(&single)
            .unwrap()
            .resolve(
                &json!({"behavior":"allow","updatedInput":{"answers":{"q1":["Alpha","Beta"]}}})
            )
            .is_err()
    );
    let mut duplicate = question();
    duplicate["params"]["questions"][0]["options"][1]["id"] = json!("a");
    assert!(Pending::capture(&duplicate).is_err());
    assert!(Pending::capture(&json!({"id":false})).is_err());
}

#[test]
fn plan_approval_requires_an_explicit_valid_decision() {
    let pending = Pending::capture(&json!({"id":"plan","method":"cursor/create_plan","params":{
        "toolCallId":"plan","plan":"Change the module"}}))
    .unwrap();
    assert_eq!(
        pending.resolve(&json!({"behavior":"allow"})).unwrap()["result"]["outcome"]["outcome"],
        "accepted"
    );
    assert_eq!(
        pending
            .resolve(&json!({"behavior":"deny","message":"Revise it"}))
            .unwrap()["result"]["outcome"]["reason"],
        "Revise it"
    );
    for response in [
        json!({"behavior":"allow","selectedActionId":"deny"}),
        json!({"behavior":"allow","updatedInput":{"plan":"changed"}}),
        json!({"behavior":"allow","extra":true}),
        json!({"behavior":"allow","interrupt":"yes"}),
    ] {
        assert!(pending.resolve(&response).is_err());
    }
}

#[test]
fn cancellations_resolve_every_callback_without_approval() {
    let tool = json!({"id":"tool","method":"session/request_permission","params":{
        "toolCall":{"toolCallId":"call","title":"shell"},"options":[
            {"optionId":"allow","name":"Allow","kind":"allow_once"}]}});
    let plan = json!({"id":"plan","method":"cursor/create_plan","params":{
        "toolCallId":"plan","plan":"Implement"}});
    for message in [tool, question(), plan] {
        let pending = Pending::capture(&message).unwrap();
        let reply = pending.cancelled();
        assert_eq!(reply["id"], message["id"]);
        assert_eq!(reply["result"]["outcome"], json!({"outcome":"cancelled"}));
    }
}
