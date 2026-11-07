use super::*;

#[tokio::test]
async fn background_refresh_failure_keeps_the_previous_native_directory() {
    let catalog = vec![json!({"name":"goal"})];
    let mut state = State {
        catalog: Some(catalog.clone()),
        ..State::default()
    };
    state
        .refresh
        .spawn(async { Err(AgentSessionError::Failed) });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !state.refresh.is_empty() {
            state.poll_refresh();
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(state.catalog, Some(catalog));
}

#[test]
fn follows_the_native_registry_grammar_without_trimming_or_rewriting_arguments() {
    for line in [
        "/goal",
        "/goal \n  keep spacing",
        "/plugin_command-2\tvalue",
    ] {
        assert!(name(line).is_some());
    }
    for line in [
        " /goal",
        "/Goal",
        "/goal:next",
        "/skill/name",
        "/2goal",
        "/goal\u{a0}value",
        "/",
    ] {
        assert!(name(line).is_none(), "{line:?}");
    }
}

#[test]
fn differentiates_unknown_commands_from_admitted_handler_errors() {
    assert!(matches!(
        acknowledged(&Value::Null),
        Err(AgentSessionError::Rejected)
    ));
    assert!(acknowledged(&json!({"commandId":"native-id","result":{"kind":"error","text":"Correct the arguments"}})).is_ok());
    assert!(acknowledged(&json!({"result":{"kind":"success"}})).is_err());
}
