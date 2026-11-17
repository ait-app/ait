use super::Phase;

#[test]
fn phases_use_stable_wire_names_and_terminal_states() {
    assert_eq!(
        serde_json::to_string(&Phase::WorkspaceReady).expect("phase serializes"),
        "\"workspace_ready\""
    );
    assert_eq!(
        serde_json::from_str::<Phase>("\"prompt_started\"").expect("phase deserializes"),
        Phase::PromptStarted
    );
    assert!(!Phase::AgentReady.is_terminal());
    assert!(Phase::Completed.is_terminal());
    assert!(Phase::Failed.is_terminal());
}
