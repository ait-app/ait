use super::super::interactions::Pending;
use crate::ports::agent_session::AgentSessionError;
use serde_json::{Value, json};

fn question() -> Value {
    json!({"event":"user-questions/request","eventId":"event-1","request":{"questions":[
        {"id":"language","header":"Language","question":"Choose a language","options":[{"label":"Rust"},{"label":"TypeScript"}]},
        {"id":"targets","question":"Which targets?","multiSelect":true,"options":[{"label":"Linux"},{"label":"macOS"}]}]}})
}

#[test]
fn questions_preserve_ids_choices_and_custom_answers_without_changing_the_prompt() {
    let pending = Pending::capture(&question(), None).unwrap();
    assert_eq!(pending.request["kind"], "question");
    let response = json!({"behavior":"allow","updatedInput":{"answers":{"language":"Rust","targets":["Linux","Windows"]}}});
    assert_eq!(
        pending.resolve(&response).unwrap(),
        json!({"kind":"result","value":{"answers":[
        {"id":"language","selected":["Rust"]},{"id":"targets","selected":["Linux"],"custom":"Windows"}]}})
    );
    assert_eq!(
        pending.resolve(&json!({"behavior":"deny"})).unwrap()["kind"],
        "rejected"
    );
}

#[test]
fn questions_reject_missing_extra_duplicate_and_single_select_multiple_answers() {
    let pending = Pending::capture(&question(), None).unwrap();
    for answers in [
        json!({"language":"Rust"}),
        json!({"language":"Rust","targets":"Linux","extra":"x"}),
        json!({"language":["Rust","TypeScript"],"targets":"Linux"}),
        json!({"language":"Rust","targets":["Linux","Linux"]}),
    ] {
        assert_eq!(
            pending
                .resolve(&json!({"behavior":"allow","updatedInput":{"answers":answers}}))
                .unwrap_err(),
            AgentSessionError::Rejected
        );
    }
    assert!(pending.resolve(&json!({"behavior":"allow","updatedInput":{"questions":[],"answers":{"language":"Rust","targets":"Linux"}}})).is_err());
    let mut invalid = question();
    invalid["request"]["questions"][1]["id"] = json!("language");
    assert!(Pending::capture(&invalid, None).is_err());
}

#[test]
fn approvals_keep_native_one_shot_authority_and_reject_input_mutations() {
    let frame = json!({"event":"approval/request","eventId":"approval-1","request":{"toolName":"bash","callId":"call","reason":"Run command"}});
    let tool = json!({"name":"bash","input":{"command":"pwd"}});
    let pending = Pending::capture(&frame, Some(&tool)).unwrap();
    assert_eq!(
        pending.resolve(&json!({"behavior":"allow"})).unwrap(),
        json!({"kind":"result","value":"allowed-once"})
    );
    assert_eq!(
        pending.resolve(&json!({"behavior":"deny"})).unwrap(),
        json!({"kind":"result","value":"rejected"})
    );
    assert!(
        pending
            .resolve(&json!({"behavior":"allow","updatedInput":{"command":"rm x"}}))
            .is_err()
    );
    assert!(
        pending
            .resolve(&json!({"behavior":"allow","selectedActionId":"allow_always"}))
            .is_err()
    );
    assert!(Pending::capture(&frame, None).is_err());
}
