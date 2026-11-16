use super::*;

#[test]
fn terminal_input_previews_are_utf8_safe_and_label_capacity_never_rejects_execution() {
    let mut terminal = Terminal::default();
    for index in 0..=MAX_COMMANDS {
        terminal
            .observe(
                "item/completed",
                &json!({"item":{"type":"commandExecution",
            "processId":index.to_string(),"command":"interactive"}}),
            )
            .unwrap();
    }
    assert!(terminal.commands.len() <= MAX_COMMANDS);
    let input = "界".repeat(PREVIEW_BYTES);
    let event = terminal
        .observe(
            "item/commandExecution/terminalInteraction",
            &json!({"turnId":"turn","processId":"new-process","stdin":input}),
        )
        .unwrap()
        .unwrap();
    let AgentTurnEvent::Timeline(entry) = event else {
        panic!("expected terminal input");
    };
    let text = entry.item["detail"]["text"].as_str().unwrap();
    assert!(text.starts_with('界'));
    assert!(text.contains("preview truncated"));
    assert!(text.len() < PREVIEW_BYTES + 100);
    assert!(entry.item["detail"].get("label").is_none());
    assert_eq!(
        terminal.observe(
            "item/commandExecution/terminalInteraction",
            &json!({"turnId":"turn","processId":"42","stdin":false})
        ),
        Err(AgentSessionError::Failed)
    );
}

#[test]
fn terminal_labels_cross_turns_and_repeated_input_remains_independent() {
    let mut terminal = Terminal::default();
    terminal
        .observe(
            "item/completed",
            &json!({"turnId":"first", "item":{
        "type":"commandExecution","processId":"42","command":"interactive"}}),
        )
        .unwrap();
    let params = json!({"turnId":"second","processId":"42","stdin":"\n"});
    let first = terminal
        .observe("item/commandExecution/terminalInteraction", &params)
        .unwrap()
        .unwrap();
    let second = terminal
        .observe("item/commandExecution/terminalInteraction", &params)
        .unwrap()
        .unwrap();
    let (AgentTurnEvent::Timeline(first), AgentTurnEvent::Timeline(second)) = (first, second)
    else {
        panic!("expected terminal display items");
    };
    assert_ne!(first.key, second.key);
    assert_eq!(first.item["detail"]["label"], "interactive");
    assert_eq!(first.item["detail"]["text"], "\n");
}
