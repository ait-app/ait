use super::*;

#[tokio::test]
async fn official_conflicting_roots_aliases_and_subcommands_never_reach_native_dispatch() {
    // Official 18.14.0 registry, including the separately imported seance command.
    // Exercise the public result and native write boundary, not the adapter's internal allowlist.
    let conflicting = "security model switch fast slow prewalk modelpreset effort trace share \
        browser stats ssh fresh shake handoff rename move wt force";
    let terminal = "settings setup plan plan-review vibe goal guided-goal loop queue collab join \
        leave copy open hotkeys extensions agents git local hub branch fork tree login logout new \
        clear delete seance resume tan omfg cleanse debug exit restart skills live record pause quit";
    let aliases = "providers models status subagents rewind worktree force: q";
    let subcommands = [
        "advisor configure",
        "session delete",
        "session pin account",
        "todo edit",
        "todo expand",
        "todo collapse",
        "mcp unauth",
        "mcp smithery-login",
        "mcp smithery-logout",
        "mcp reconnect",
        "mcp notifications",
    ];
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    let catalog: Vec<_> = conflicting
        .split_whitespace()
        .chain(["advisor", "session", "todo", "mcp"])
        .map(|name| json!({"name":name,"source":"builtin"}))
        .collect();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    while session.poll_turn().unwrap().is_some() {}
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read(&calls).unwrap();
    for input in conflicting
        .split_whitespace()
        .chain(terminal.split_whitespace())
        .chain(aliases.split_whitespace())
        .chain(subcommands)
    {
        let text = format!("/{input}");
        session
            .out_of_band(&AgentPrompt::text(&text))
            .await
            .unwrap();
        let event = next(session.as_mut()).await.unwrap();
        assert!(
            matches!(event, AgentTurnEvent::Timeline(entry)
                if entry.item["type"] == "provider_command"
                    && entry.item["status"] == "error"
                    && entry.item["text"].as_str().is_some_and(|text| !text.is_empty())),
            "{text} must produce an independent refusal, not a model turn"
        );
        assert!(session.take_command_completion().is_none());
    }
    assert_eq!(
        std::fs::read(&calls).unwrap(),
        before,
        "no rejected root, alias or subcommand may write a native prompt or control request"
    );
    session.close().await.unwrap();
}
