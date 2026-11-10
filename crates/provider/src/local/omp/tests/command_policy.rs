use super::*;

#[tokio::test]
async fn official_conflicting_roots_aliases_and_subcommands_never_reach_native_dispatch() {
    // Official 18.14.0 registry, including the separately imported seance command.
    // Exercise the public result and native write boundary, not the adapter's internal allowlist.
    let conflicting = "model switch fast slow prewalk modelpreset effort trace share \
        browser stats ssh fresh shake rename move wt";
    let terminal = "settings setup plan plan-review vibe goal guided-goal loop queue collab join \
        leave copy open hotkeys extensions agents git local hub branch fork tree login logout new \
        clear delete seance resume tan omfg cleanse debug exit restart skills live record pause quit";
    let aliases = "providers models status subagents rewind worktree q";
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

#[tokio::test]
async fn effective_plugin_owners_keep_menu_and_dispatch_for_builtin_and_host_special_names() {
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/missing-preset-builtin"), "true").unwrap();
    let names = ["record", "modelpreset", "effort", "vibe", "btw", "goal"];
    let catalog: Vec<_> = names
        .iter()
        .map(|name| json!({"name":name,"source":"extension","description":"Project command"}))
        .collect();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    let discovered = client.commands(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    for name in names {
        assert!(
            discovered
                .iter()
                .any(|entry| entry["name"] == name && entry["source"] == "extension")
        );
        assert!(
            session
                .commands()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == name && entry["source"] == "extension")
        );
        let text = format!("/{name} project argument");
        assert_eq!(session.handles_out_of_band(&text), Some(false), "{name}");
        session.start_turn(&text, &spec.config).await.unwrap();
        let question = next(session.as_mut()).await.unwrap();
        assert!(
            matches!(question, AgentTurnEvent::PermissionRequested(_)),
            "{name}: {question:?}"
        );
        session
            .respond_permission(
                "plugin-question",
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":"A, B"}}}),
            )
            .await
            .unwrap();
        let events = tokio::time::timeout(Duration::from_secs(4), complete(session.as_mut()))
            .await
            .unwrap_or_else(|_| panic!("plugin {name} did not settle"));
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    }
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let prompts: Vec<Value> = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| call["type"] == "prompt")
        .collect();
    assert_eq!(prompts.len(), names.len());
    for (call, name) in prompts.iter().zip(names) {
        assert_eq!(call["message"], format!("/{name} project argument"));
    }
    session.close().await.unwrap();
}
