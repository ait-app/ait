use super::*;

#[tokio::test]
async fn acp_preserves_command_metadata_and_publishes_live_native_changes() {
    let (root, client, spec) = crate::local::omp::tests::fixture();
    let native: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/omp_commands.json"
    ))
    .unwrap();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        native["commands"].to_string(),
    )
    .unwrap();
    let mut engine = engine::Engine::new(client);
    engine
        .dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":1}}))
        .await;
    let opened = engine
        .dispatch(json!({"jsonrpc":"2.0","id":2,"method":"session/new",
        "params":{"cwd":spec.cwd,"mcpServers":[]}}))
        .await;
    let update = opened
        .iter()
        .find(|message| message["params"]["update"]["sessionUpdate"] == "available_commands_update")
        .unwrap();
    let advisor = update["params"]["update"]["availableCommands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == "advisor")
        .unwrap();
    assert_eq!(
        advisor["_meta"]["aliases"],
        native["commands"][0]["aliases"]
    );
    let headless_subcommands = native["commands"][0]["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|command| command["name"] != "configure")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(advisor["_meta"]["subcommands"], json!(headless_subcommands));
    assert_eq!(
        advisor["input"]["hint"],
        native["commands"][0]["input"]["hint"]
    );
    let session = opened.last().unwrap()["result"]["sessionId"].clone();
    std::fs::write(root.path().join("agent/catalog-update.json"), "[]").unwrap();
    engine.dispatch(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt",
        "params":{"sessionId":session,"prompt":[{"type":"text","text":"/fixture-command-update"}]}})).await;
    let messages = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut messages = Vec::new();
        loop {
            messages.extend(engine.poll().unwrap());
            if messages.iter().any(|message| message["id"] == 3) {
                break messages;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let updates: Vec<_> = messages
        .iter()
        .filter(|message| {
            message["params"]["update"]["sessionUpdate"] == "available_commands_update"
        })
        .collect();
    assert_eq!(updates.len(), 1);
    let commands = &updates[0]["params"]["update"]["availableCommands"];
    assert_eq!(commands.as_array().unwrap().len(), 1);
    assert_eq!(commands[0]["name"], "btw");
    engine.close().await.unwrap();
}
