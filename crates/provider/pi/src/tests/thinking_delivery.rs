use super::*;

#[test]
fn catalog_off_is_a_metadata_hint_separate_from_native_clamping() {
    for (capabilities, offered) in [
        (json!({}), false),
        (json!({"thinkingLevelMap":{"off":"none"}}), true),
        (json!({"thinkingLevelMap":{"off":null}}), false),
        (json!({"thinkingLevelMap":{"off":false}}), false),
        (json!({"thinkingLevelMap":{"off":42}}), false),
        (json!({"thinkingLevelMap":{"off":""}}), false),
        (json!({"thinkingLevelMap":{"high":"high"}}), false),
        (
            json!({"compat":{"thinkingFormat":"future-native-disable"}}),
            false,
        ),
        (
            json!({"compat":{"supportsReasoningEffort":false},"thinkingLevelMap":{"off":"none"}}),
            true,
        ),
        (
            json!({"provider":"deepseek","baseUrl":"https://api.deepseek.com/v1"}),
            false,
        ),
        (
            json!({"provider":"openrouter","baseUrl":"https://openrouter.ai/v1"}),
            false,
        ),
        (json!({"api":"anthropic-messages"}), true),
        (json!({"api":"openai-responses"}), true),
    ] {
        let mut model = json!({"provider":"backend","id":"reasoner",
            "api":"openai-completions","reasoning":true});
        model
            .as_object_mut()
            .unwrap()
            .extend(capabilities.as_object().unwrap().clone());
        let models = config::models(
            &json!({"models":[model.clone()]}),
            &json!({"thinkingLevel":"off"}),
        )
        .unwrap();
        let options = models[0]["thinkingOptions"].as_array().unwrap();
        assert_eq!(
            options.iter().any(|option| option["id"] == "off"),
            offered,
            "{capabilities}"
        );
        assert_eq!(
            models[0]["defaultThinkingOptionId"],
            if offered { "off" } else { "minimal" }
        );
        // The picker must not become a second validation or restoration authority.
        let native = if model["thinkingLevelMap"].get("off") == Some(&Value::Null) {
            "minimal"
        } else {
            "off"
        };
        assert_eq!(
            config::clamp_thinking_level(&model, "off").as_deref(),
            Some(native)
        );
        for option in options.iter().filter(|option| option["id"] == "off") {
            assert!(
                option["description"]
                    .as_str()
                    .unwrap()
                    .contains("not verified")
            );
        }
    }
}

#[test]
fn catalog_rejects_unusable_off_mappings_for_every_api() {
    for api in [
        Value::Null,
        json!("openai-completions"),
        json!("openai-responses"),
        json!("anthropic-messages"),
        json!("future-api"),
    ] {
        for (mapping, offered) in [
            (Value::Null, false),
            (json!(false), false),
            (json!(42), false),
            (json!(""), false),
            (json!([]), false),
            (json!({}), false),
            (json!("none"), true),
        ] {
            let models = config::models(
                &json!({"models":[{"provider":"backend","id":"model","reasoning":true,
                    "api":api,"thinkingLevelMap":{"off":mapping}}]}),
                &json!({"thinkingLevel":"off"}),
            )
            .unwrap();

            assert_eq!(
                models[0]["thinkingOptions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|option| option["id"] == "off"),
                offered,
                "api={api}, off={mapping}"
            );
            assert_eq!(
                models[0]["defaultThinkingOptionId"],
                if offered { "off" } else { "minimal" }
            );
        }
    }
}

#[tokio::test]
async fn hidden_off_is_forwarded_during_creation_resume_and_model_changes() {
    let (directory, client, mut spec) = fixture("completions-unmapped-off");
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(
        details.models[0]["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|option| option["id"] != "off")
    );
    spec.config.thinking_option_id = Some("off".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("off")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("off")
    );
    session.start_turn("resumed", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    spec.config.model = Some("fixture/other".into());
    session
        .start_turn("after switch", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("off")
    );
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        commands
            .iter()
            .filter(|command| command["type"] == "prompt")
            .count(),
        3
    );
    let levels: Vec<_> = commands
        .iter()
        .filter(|command| command["type"] == "set_thinking_level")
        .map(|command| command["level"].clone())
        .collect();
    assert_eq!(levels, [json!("off"), json!("off"), json!("off")]);
}

#[tokio::test]
async fn native_off_is_reported_without_a_delivery_claim() {
    let (_directory, client, spec) = fixture("completions-native-off");
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "minimal");
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("off")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn catalog_metadata_changes_do_not_rewrite_native_facts_or_block_prompts() {
    let (directory, client, mut spec) = fixture("completions-mapped-off");
    spec.config.thinking_option_id = Some("off".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    for hidden in [true, false] {
        let marker = directory.path().join("disable-off");
        if hidden {
            std::fs::write(&marker, "").unwrap();
        } else {
            std::fs::remove_file(&marker).unwrap();
        }
        session
            .start_turn("unchanged preference", &spec.config)
            .await
            .unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::RuntimeInfo(_)))
        );
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("off")
        );
        assert!(session.failure_diagnostic().is_none());
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_rejection_still_blocks_off_and_allows_correction() {
    let (directory, client, mut spec) = fixture("completions-reject-off");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("off".into());
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_configuration"
    );
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .any(|command| command["type"] == "set_thinking_level" && command["level"] == "off")
    );
    assert!(commands.iter().all(|command| command["type"] != "prompt"));
    spec.config.thinking_option_id = Some("low".into());
    session.start_turn("keep this", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}
