use super::*;

#[test]
fn command_parsing_never_interprets_unrelated_text_as_a_native_mutation() {
    assert!(out_of_band(" /goal pause "));
    assert!(!out_of_band("discuss /goal pause"));
    assert!(!out_of_band("/folder/goal pause"));
    assert_eq!(
        goal("thread", "PAUSE").1,
        json!({"threadId":"thread","status":"paused"})
    );
    assert_eq!(goal("thread", "resume").1["status"], "active");
    assert_eq!(goal("thread", "clear").0, Some("thread/goal/clear"));
    assert_eq!(
        goal("thread", "ship feature").1["objective"],
        "ship feature"
    );
    assert!(goal("thread", "").0.is_none());
}

#[test]
fn goal_edits_are_explicit_and_do_not_resume_a_paused_goal() {
    assert!(goal("thread", "set").0.is_none());
    let (_, params, _) = goal("thread", "set pause");
    assert_eq!(params["objective"], "pause");
    assert!(params.get("status").is_none());
}
