use super::*;

#[test]
fn goals_use_native_rpc_operations_without_reinterpreting_budget_commands() {
    assert_eq!(
        goal_arguments("Ship").unwrap(),
        json!({"op":"create","objective":"Ship"})
    );
    assert_eq!(goal_arguments("set pause").unwrap()["objective"], "pause");
    assert_eq!(goal_arguments("pause").unwrap()["op"], "pause");
    assert_eq!(goal_arguments("resume").unwrap()["op"], "resume");
    assert_eq!(goal_arguments("clear").unwrap()["op"], "drop");
    assert_eq!(goal_arguments("show").unwrap()["op"], "get");
    assert!(goal_arguments("budget 100").is_err());
}
