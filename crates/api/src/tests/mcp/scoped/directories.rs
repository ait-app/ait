use super::*;

#[tokio::test]
async fn scoped_tools_hide_directory_arguments_and_keep_bot_only_ticket_assignment() {
    let scoped = Scoped::start().await;
    scoped
        .setup(json!({"groups":["observe","agents","schedules","terminals"],"modes":{"codex":["auto"]}}))
        .await;
    let bot = scoped.bot_credential();
    let child = scoped.subtree_agent().await;
    let subtree = scoped.subtree_credential(&child);

    for credential in [TOKEN, bot.as_str(), subtree.as_str()] {
        let listed: Value = scoped
            .post(credential, None, "tools/list", json!({}))
            .await
            .json()
            .await
            .unwrap();
        let tools = listed["result"]["tools"].as_array().unwrap();
        for name in [
            "agent_create",
            "terminal_create",
            "terminal_list",
            "schedule_create",
            "schedule_update",
            "provider_model_list",
            "provider_get",
        ] {
            let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
            let properties = &tool["inputSchema"]["properties"];
            assert_eq!(
                properties.get("cwd").is_some(),
                credential == TOKEN,
                "{name}"
            );
            if name == "agent_create" {
                assert_eq!(properties.get("ticketId").is_some(), credential == bot);
            }
            if credential != TOKEN && !name.starts_with("schedule_") {
                assert!(tool["description"].as_str().unwrap().contains("Omit cwd"));
            }
        }
    }
    scoped.stop().await;
}

#[tokio::test]
async fn scoped_agent_creation_recovers_from_directory_and_model_argument_errors() {
    let scoped = Scoped::start().await;
    scoped
        .setup(json!({"groups":["observe","agents","terminals"],"modes":{"codex":["auto"]}}))
        .await;
    let bot = scoped.bot_credential();
    for (tool, arguments) in [
        ("terminal_create", json!({"cwd":scoped.cwd})),
        (
            "provider_model_list",
            json!({"provider":"codex","cwd":scoped.cwd}),
        ),
    ] {
        let denied = scoped.call(&bot, None, tool, arguments).await;
        assert_eq!(denied["error"]["code"], -32602, "{denied}");
        assert!(
            denied["error"]["message"]
                .as_str()
                .unwrap()
                .contains("omit cwd")
        );
    }
    let mut arguments = json!({"provider":"codex","title":"Inspect errors","initialPrompt":"work","cwd":scoped.cwd});
    let directory_error = scoped
        .call(&bot, None, "create_agent", arguments.clone())
        .await;
    assert_eq!(directory_error["error"]["code"], -32602);
    assert!(
        directory_error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("omit cwd")
    );

    arguments.as_object_mut().unwrap().remove("cwd");
    let model_error = scoped
        .call(&bot, None, "agent_create", arguments.clone())
        .await;
    assert_eq!(model_error["error"]["code"], -32602);
    assert!(
        model_error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("provider/model")
    );
    let models = scoped
        .call(
            &bot,
            None,
            "provider_model_list",
            json!({"provider":"codex"}),
        )
        .await;
    structured(&models);
    arguments["provider"] = json!("codex/offline-model");
    let created = scoped.call(&bot, None, "agent_create", arguments).await;
    assert!(structured(&created)["agent"]["id"].is_string(), "{created}");
    scoped.stop().await;
}
