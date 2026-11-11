use super::*;

fn fast_toggle(session: &dyn AgentSession, config: &StoredAgentConfig) -> Option<Value> {
    session.control_settings(config).unwrap()["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == "fastMode")
        .cloned()
}

fn with_fast(config: &StoredAgentConfig, enabled: bool) -> StoredAgentConfig {
    StoredAgentConfig {
        feature_values: Some(BTreeMap::from([("fastMode".to_owned(), json!(enabled))])),
        ..config.clone()
    }
}

#[tokio::test]
async fn catalog_models_use_paseo_labels_and_thinking_options() {
    let (root, client, spec) = fixture();
    let details = client.discover(&spec.cwd).await.unwrap();
    let ids = |model: &Value| -> Vec<String> {
        model["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["id"].as_str().unwrap().to_owned())
            .collect()
    };

    assert_eq!(details.models.len(), 4);
    assert_eq!(details.models[0]["id"], r#"["test","model"]"#);
    assert_eq!(details.models[0]["label"], "Fixture model");
    assert_eq!(details.models[0]["description"], "test/model");
    assert_eq!(
        details.models[0]["metadata"],
        json!({"provider":"test","modelId":"model"})
    );
    assert_eq!(details.models[0]["isDefault"], true);
    assert_eq!(ids(&details.models[0]), ["auto", "low", "high"]);
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "low");
    // OMP's reported default level wins when the model lists it.
    assert_eq!(ids(&details.models[1]), ["auto", "low", "high", "max"]);
    assert_eq!(details.models[1]["defaultThinkingOptionId"], "high");
    assert_eq!(details.models[1]["isDefault"], false);
    // No per-model efforts: every level, default medium.
    assert_eq!(details.models[2]["label"], "Any effort");
    assert_eq!(
        ids(&details.models[2]),
        [
            "auto", "off", "minimal", "low", "medium", "high", "xhigh", "max"
        ]
    );
    assert_eq!(details.models[2]["defaultThinkingOptionId"], "medium");
    // Non-reasoning: no thinking options.
    assert_eq!(details.models[3]["label"], "Plain");
    assert!(details.models[3].get("thinkingOptions").is_none());
    assert!(details.models[3].get("defaultThinkingOptionId").is_none());
    assert!(details.features.is_empty());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("get_available_thinking_levels"));
    assert!(!calls.contains("set_fast_mode"));
}

#[test]
fn modes_are_dynamic_while_mcp_and_rewind_stay_deferred() {
    let (_root, client, spec) = fixture();
    let capabilities = &client.settings(&spec.config)["capabilities"];

    assert_eq!(capabilities["supportsDynamicModes"], true);
    for deferred in [
        "supportsMcpServers",
        "supportsRewindConversation",
        "supportsRewindFiles",
        "supportsRewindBoth",
    ] {
        assert_eq!(capabilities[deferred], false, "{deferred}");
    }
}

#[tokio::test]
async fn drafts_offer_no_controls_and_start_no_process() {
    let (root, client, mut spec) = fixture();
    spec.config = with_fast(&spec.config, true);
    spec.config.model = Some(json!(["unknown", "model"]).to_string());

    assert!(client.draft_features(&spec).await.unwrap().is_empty());
    assert!(
        client.settings(&spec.config)["features"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    spec.config.model = Some("unknown/model".to_owned());
    assert_eq!(
        client.draft_features(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(!root.path().join("agent/calls.jsonl").exists());
}

#[tokio::test]
async fn fast_is_restored_at_launch_and_follows_live_model_changes() {
    let (root, client, mut spec) = fixture();
    spec.config = with_fast(&spec.config, true);
    let mut session = client.create_session(&spec).await.unwrap();
    let toggle = fast_toggle(session.as_ref(), &spec.config).unwrap();
    assert_eq!(toggle["value"], true);
    assert!(toggle.get("description").is_none());

    spec.config.model = Some(json!(["other", "model"]).to_string());
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let toggle = fast_toggle(session.as_ref(), &spec.config).unwrap();
    assert_eq!(toggle["value"], true);
    assert_eq!(
        toggle["description"],
        "Fast is enabled but does not apply to this model."
    );

    let pending = with_fast(&spec.config, false);
    assert_eq!(
        fast_toggle(session.as_ref(), &pending).unwrap()["value"],
        false
    );
    session.start_turn("hello", &pending).await.unwrap();
    complete(session.as_mut()).await;
    let toggle = fast_toggle(session.as_ref(), &pending).unwrap();
    assert_eq!(toggle["value"], false);
    assert!(toggle.get("description").is_none());
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let fast: Vec<_> = calls
        .lines()
        .filter(|line| line.contains("\"set_fast_mode\""))
        .collect();
    assert_eq!(fast.len(), 2);
    assert!(fast[0].contains("\"enabled\": true"));
    assert!(fast[1].contains("\"enabled\": false"));
    assert_eq!(calls.matches("\"set_model\"").count(), 1);
    assert!(!calls.contains("set_slow_mode") && !calls.contains("set_auto_"));
}

#[tokio::test]
async fn a_refused_fast_selection_keeps_omps_value_without_failing() {
    let (_root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    spec.config = with_fast(&spec.config, true);

    let mut session = client.create_session(&spec).await.unwrap();

    assert_eq!(
        fast_toggle(session.as_ref(), &spec.config).unwrap()["value"],
        false
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn fast_is_hidden_when_omp_does_not_report_it() {
    let (root, client, mut spec) = fixture_with(&[("OMP_FIXTURE_NO_FAST_STATE", "1")]);
    spec.config = with_fast(&spec.config, true);

    let mut session = client.create_session(&spec).await.unwrap();

    assert!(fast_toggle(session.as_ref(), &spec.config).is_none());
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("set_fast_mode"));
}

#[tokio::test]
async fn images_reach_text_only_models_too_and_provider_options_reach_the_launch() {
    let (root, client, mut spec) = fixture();
    spec.config.provider_options = Some(BTreeMap::from([
        ("smolModel".to_owned(), json!("test/plain")),
        ("rpcTimeoutMs".to_owned(), json!(30_000)),
    ]));
    let image = |text: &str| AgentPrompt {
        text: text.to_owned(),
        images: vec![PromptImage {
            data: "aW1hZ2U=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..AgentPrompt::default()
    };
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_input(&image("see"), &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    // `other/model` reports text input only.
    spec.config.model = Some(json!(["other", "model"]).to_string());
    session
        .start_input(&image("again"), &spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    // OMP describes the image to a text-only model in a hidden message, which Ait omits.
    let rows: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.item.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !rows.iter().any(|row| row.to_string().contains("vision")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row["type"] == "user_message"),
        "{rows:?}"
    );
    session.start_turn("plain", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    // Provider options are launch arguments, so a change relaunches OMP.
    spec.config.provider_options = None;
    session
        .start_turn("relaunched", &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    // History omits the hidden description too, and keeps the user's image.
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert!(
        !history
            .iter()
            .any(|entry| entry.item.to_string().contains("vision"))
    );
    let again = history
        .iter()
        .find(|entry| {
            entry.item["type"] == "user_message" && entry.item["text"].to_string().contains("again")
        })
        .unwrap_or_else(|| panic!("{history:?}"));
    assert!(
        again.item["text"].to_string().contains("![Image]"),
        "{again:?}"
    );
    let calls = calls_after(root.path(), 0);
    let prompts: Vec<_> = calls
        .iter()
        .filter(|call| call["type"] == "prompt")
        .collect();
    for (prompt, text) in prompts[..2].iter().zip(["see", "again"]) {
        assert_eq!(prompt["message"], text);
        assert_eq!(
            prompt["images"],
            json!([{"type":"image","data":"aW1hZ2U=","mimeType":"image/png"}])
        );
    }
    for prompt in &prompts[2..] {
        assert!(prompt.get("images").is_none(), "{prompt}");
    }
    let launches = launches(&calls);
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0][4..6], ["--smol", "test/plain"]);
    assert!(!launches[1].contains(&"--smol".to_owned()));
}
