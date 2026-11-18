use super::*;

const FLASH: &str = "m:minimax:MiniMax-M3.1-Flash";
const M3: &str = "m:minimax:MiniMax-M3";

fn variants() -> (tempfile::TempDir, MiniMaxClient, AgentSessionSpec) {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "variants").unwrap();
    (root, client, spec)
}

async fn runtime(
    client: &MiniMaxClient,
    spec: &AgentSessionSpec,
) -> Result<(Option<String>, Option<String>), AgentSessionError> {
    let mut session = client.create_session(spec).await?;
    let info = session.runtime_info().await.unwrap();
    session.close().await.unwrap();
    Ok((info.model, info.thinking_option_id))
}

fn requested_models(root: &tempfile::TempDir) -> Vec<String> {
    std::fs::read_to_string(root.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| request["params"]["configId"] == "model")
        .map(|request| request["params"]["value"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn discovery_lists_each_model_once_with_variant_thinking_options() {
    let (_root, client, spec) = variants();

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(
        details.models,
        vec![
            json!({"provider":PROVIDER,"id":FLASH,"label":"M3.1-Flash","isDefault":true,
                "isSelectable":true,"defaultThinkingOptionId":"thinking:default",
                "thinkingOptions":[
                    {"id":"thinking:default","label":"Default","isDefault":true},
                    {"id":"thinking:low","label":"Low","isDefault":false}]}),
            json!({"provider":PROVIDER,"id":M3,"label":"M3","isDefault":false,
                "isSelectable":true,"defaultThinkingOptionId":"thinking",
                "thinkingOptions":[
                    {"id":"off","label":"Off","isDefault":false},
                    {"id":"thinking","label":"Thinking","isDefault":true}]}),
        ]
    );
}

#[tokio::test]
async fn every_discovered_thinking_option_selects_its_native_variant() {
    let (root, client, mut spec) = variants();
    let details = client.discover(&spec.cwd).await.unwrap();
    for model in &details.models {
        for option in model["thinkingOptions"].as_array().unwrap() {
            spec.config.model = model["id"].as_str().map(str::to_owned);
            spec.config.thinking_option_id = option["id"].as_str().map(str::to_owned);

            let selected = runtime(&client, &spec).await.unwrap();

            assert_eq!(
                selected,
                (
                    spec.config.model.clone(),
                    spec.config.thinking_option_id.clone()
                )
            );
        }
    }
    assert!(requested_models(&root).contains(&format!("{M3}:v:")));
}

#[tokio::test]
async fn a_model_without_thinking_falls_back_past_rejected_variants() {
    let (root, client, mut spec) = variants();
    spec.config.model = Some(M3.into());
    spec.config.thinking_option_id = Some("off".into());
    let mut session = client.create_session(&spec).await.unwrap();
    let flash = StoredAgentConfig {
        model: Some(FLASH.into()),
        ..StoredAgentConfig::default()
    };

    session.start_turn("hello", &flash).await.unwrap();

    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some(FLASH));
    assert_eq!(info.thinking_option_id.as_deref(), Some("thinking:default"));
    assert!(
        requested_models(&root).ends_with(&[format!("{FLASH}:v:"), format!("{FLASH}:v:thinking")])
    );
    finish(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn stored_native_variants_and_unknown_thinking_identities() {
    let (_root, client, mut spec) = variants();
    spec.config.model = Some(format!("{FLASH}:v:thinking"));
    spec.config.thinking_option_id = Some("low".into());
    assert_eq!(
        runtime(&client, &spec).await.unwrap(),
        (Some(FLASH.into()), Some("thinking:low".into()))
    );
    spec.config.model = Some(M3.into());
    spec.config.thinking_option_id = None;
    assert_eq!(
        runtime(&client, &spec).await.unwrap(),
        (Some(M3.into()), Some("thinking".into()))
    );

    for (model, thinking) in [
        (FLASH, "off"),
        (FLASH, "thinking:ultra"),
        (M3, "thinking:low"),
        (M3, "fast"),
        ("m:minimax:MiniMax-M9", "thinking"),
    ] {
        spec.config.model = Some(model.into());
        spec.config.thinking_option_id = Some(thinking.into());

        let result = runtime(&client, &spec).await;

        assert!(
            matches!(result, Err(AgentSessionError::Rejected)),
            "{model} {thinking}"
        );
    }
}

#[test]
fn resolve_orders_candidates_and_decodes_thinking_identities() {
    let native = json!({"currentValue":format!("{M3}:v:"),"options":[
        {"value":format!("{FLASH}:v:"),"name":"M3.1-Flash"},
        {"value":format!("{FLASH}:v:thinking"),"name":"M3.1-Flash · thinking"},
        {"value":"plain","name":"Plain"}]});
    let resolve = |model, thinking| catalog::resolve(&native, model, thinking);

    assert_eq!(
        resolve(Some(FLASH), None).unwrap().models,
        [format!("{FLASH}:v:"), format!("{FLASH}:v:thinking")]
    );
    assert_eq!(
        resolve(Some(FLASH), Some("thinking:max")).unwrap(),
        catalog::Selection {
            models: vec![format!("{FLASH}:v:thinking")],
            effort: Some("max".into())
        }
    );
    assert_eq!(
        resolve(Some("plain"), Some("high")).unwrap(),
        catalog::Selection {
            models: vec!["plain".into()],
            effort: Some("high".into())
        }
    );
    assert_eq!(
        resolve(None, None).unwrap(),
        catalog::Selection {
            models: Vec::new(),
            effort: None
        }
    );
    assert!(matches!(
        resolve(None, Some("thinking")),
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(
        catalog::thinking_id(Some(""), Some("low")).unwrap(),
        "off:low"
    );
    assert_eq!(catalog::thinking_id(None, None), None);
}
