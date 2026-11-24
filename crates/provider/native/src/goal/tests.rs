use super::*;

#[test]
fn native_goals_preserve_facts_and_declare_only_supported_actions() {
    let goal = json!({"objective":" Ship ","status":"active","tokenBudget":100,"tokensUsed":12});
    let value = snapshot(&goal, true).unwrap();
    assert_eq!(value["objective"], "Ship");
    assert_eq!(value["tokenBudget"], 100);
    assert_eq!(value["tokensUsed"], 12);
    assert_eq!(value["actions"]["edit"], true);
    assert_eq!(value["actions"]["pause"], true);
    assert_eq!(
        snapshot(&json!({"goal":goal}), false).unwrap()["actions"]["edit"],
        false
    );
    assert_eq!(
        snapshot(&json!({"objective":"Ship","status":"paused"}), false).unwrap()["actions"]["resume"],
        true
    );
    assert_eq!(snapshot(&Value::Null, true), Some(Value::Null));
    assert_eq!(
        snapshot(&json!({"status":"dropped"}), false),
        Some(Value::Null)
    );
    assert_eq!(
        snapshot(&json!({"objective":"","status":"active"}), false),
        None
    );
    assert_eq!(snapshot(&json!({"objective":"Ship"}), false), None);
}

#[test]
fn native_goal_terminal_and_limited_states_keep_supported_actions() {
    for status in [
        "blocked",
        "usageLimited",
        "budgetLimited",
        "budget_exhausted",
    ] {
        let value = snapshot(&json!({"objective":"Ship","status":status}), true).unwrap();
        assert_eq!(value["actions"]["pause"], false);
        assert_eq!(value["actions"]["resume"], true);
        assert_eq!(value["actions"]["clear"], true);
    }
    let complete = snapshot(&json!({"objective":"Ship","status":"complete"}), true).unwrap();
    assert_eq!(complete["status"], "completed");
    assert_eq!(complete["actions"]["resume"], false);
}
