use super::*;

fn snapshot(phase: &str) -> Value {
    json!({"version":1,"operation":"pause","goal":{"id":"native-goal","revision":2,
        "phase":phase,"objective":"Keep the objective intact","maxGoalRounds":256},
        "roundsStarted":1})
}

#[test]
fn command_cards_use_native_goal_facts_and_keep_original_output_as_details() {
    let mut state = State::default();
    state
        .apply(
            &json!({"type":"command/run","data":{"commandId":"control","name":"goal"}}),
            "session",
        )
        .unwrap();
    assert!(
        state
            .apply(
                &json!({"type":"goal/change","seq":2,"data":snapshot("paused")}),
                "session"
            )
            .unwrap()
            .is_none()
    );
    let item = state
        .apply(
            &json!({"type":"command/done","data":{"commandId":"control",
        "kind":"success","text":"Arbitrary localized native output"}}),
            "session",
        )
        .unwrap()
        .unwrap();
    assert_eq!(item["goal"]["phase"], "paused");
    assert_eq!(item["goal"]["rounds"], 1);
    assert_eq!(item["text"], "Arbitrary localized native output");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/dsh_goal_control.json"
    ))
    .unwrap();
    assert_eq!(item, expected);
    assert_eq!(
        item["actions"][0],
        json!({"id":"resume","command":"/goal resume"})
    );
    assert!(
        !item["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "pause")
    );
}

#[test]
fn autonomous_completion_and_clear_are_visible_without_a_command_or_model_message() {
    let mut state = State::from_projection(&snapshot("active")).unwrap();
    let completed = state
        .apply(
            &json!({"type":"goal/change","seq":3,"data":snapshot("complete")}),
            "session",
        )
        .unwrap()
        .unwrap();
    assert_eq!(completed["goal"]["phase"], "complete");
    assert!(
        !completed["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "resume")
    );
    let cleared = state
        .apply(
            &json!({"type":"goal/change","seq":4,
        "data":{"version":1,"operation":"clear"}}),
            "session",
        )
        .unwrap()
        .unwrap();
    assert!(cleared["goal"].is_null());
    assert!(cleared["actions"].as_array().unwrap().is_empty());
    assert!(State::from_projection(&snapshot("invalid")).is_err());
}
