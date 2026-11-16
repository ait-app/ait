use super::*;
use crate::transport::{self, Transport};

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; uses only isolated models and control RPCs"]
async fn installed_pi_clamping_and_model_resets_match_host_projection() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let client = clamping_client(&root);
    transport::version(&client).await.unwrap();
    let mut transport = Transport::spawn(
        &client,
        root.to_str().unwrap(),
        &[
            "--no-session".into(),
            "--provider".into(),
            "ait-fixture".into(),
            "--model".into(),
            "baseline".into(),
        ],
        None,
    )
    .unwrap();
    let data = transport
        .request(json!({"type":"get_available_models"}))
        .await
        .unwrap();
    let native_models: Vec<_> = data["models"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|model| model["provider"] == "ait-fixture")
        .collect();
    assert_eq!(native_models.len(), 5);

    // Pi announces a model's default before acknowledging the switch.
    transport
        .request(json!({"type":"set_thinking_level","level":"low"}))
        .await
        .unwrap();
    transport.drain_pending().for_each(drop);
    transport
        .request(json!({"type":"set_model","provider":"ait-fixture","modelId":"upward"}))
        .await
        .unwrap();
    let notifications: Vec<_> = transport.drain_pending().collect();
    assert!(
        notifications
            .iter()
            .any(|event| event["type"] == "thinking_level_changed" && event["level"] == "high"),
        "{notifications:?}"
    );

    for model in native_models {
        transport
            .request(json!({"type":"set_model","provider":"ait-fixture","modelId":model["id"]}))
            .await
            .unwrap();
        for seed in [
            "off", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ] {
            transport
                .request(json!({"type":"set_thinking_level","level":seed}))
                .await
                .unwrap();
            let state = transport
                .request(json!({"type":"get_state"}))
                .await
                .unwrap();
            transport.drain_pending().for_each(drop);

            assert_eq!(
                config::clamp_thinking_level(model, seed).as_deref(),
                state["thinkingLevel"].as_str(),
                "model={}, seed={seed}",
                model["id"]
            );
            let catalog =
                config::models(&json!({"models":[model]}), &json!({"thinkingLevel":seed})).unwrap();
            if model["reasoning"] == true {
                assert_eq!(
                    catalog[0]["defaultThinkingOptionId"], state["thinkingLevel"],
                    "model={}, seed={seed}",
                    model["id"]
                );
            } else {
                assert!(catalog[0].get("defaultThinkingOptionId").is_none());
            }
        }
    }
    transport.close().await.unwrap();
}

fn clamping_client(root: &std::path::Path) -> PiClient {
    let native = root.join("native");
    std::fs::create_dir(&native).unwrap();
    let models: Vec<Value> = [
        ("baseline", true, json!({"off":"none"})),
        (
            "full-ladder",
            true,
            json!({"off":"none","xhigh":"xhigh","max":"max","ultra":"ultra"}),
        ),
        (
            "upward",
            true,
            json!({"off":null,"minimal":null,"low":null,"medium":null,
            "high":"high","xhigh":null,"max":"max"}),
        ),
        (
            "downward",
            true,
            json!({"off":"none","minimal":null,"low":"low",
            "medium":null,"high":null,"xhigh":null,"max":null}),
        ),
        ("no-reasoning", false, json!({})),
    ]
    .into_iter()
    .map(|(id, reasoning, mapping)| {
        json!({"id":id,"reasoning":reasoning,"thinkingLevelMap":mapping,
            "input":["text"],"contextWindow":8192,"maxTokens":128})
    })
    .collect();
    std::fs::write(
        native.join("models.json"),
        json!({"providers":{"ait-fixture":{
            "baseUrl":"http://127.0.0.1:1/v1","api":"openai-completions",
            "apiKey":"fixture-only","models":models
        }}})
        .to_string(),
    )
    .unwrap();
    let mut client = PiClient::new("pi".into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "PI_CODING_AGENT_DIR".into(),
            native.to_str().unwrap().into(),
        ),
        ("HOME".into(), root.to_str().unwrap().into()),
    ]))
    .unwrap();
    client
}
