use super::*;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; native role resolution with an offline model stub"]
async fn installed_omp_roles_resolve_inside_native_extensions_and_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(agent.join("extensions")).unwrap();
    let (base_url, _server) = model_stub().await;
    let (defaults, models) = native_profile(&agent, &base_url);
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into())
    };
    let mut spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: selected_roles(),
    };
    let details = client.draft_features(&spec).await.unwrap();
    assert!(
        details
            .iter()
            .any(|feature| feature["id"] == "modelRole.custom")
    );
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        Some(json!(["ait-role-fixture", "fast"]).to_string())
    );
    session
        .start_turn("Create a persistent role fixture", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Completed(Some(
            "Role fixture ready".to_owned()
        )))
    );
    session
        .start_turn("/ait-role-check", &spec.config)
        .await
        .unwrap();
    assert_native_roles(
        &complete(session.as_mut()).await,
        "ait-role-fixture/advisor",
    );
    let handle = session.persistence().unwrap();
    spec.config
        .feature_values
        .as_mut()
        .unwrap()
        .insert("modelRole.advisor".to_owned(), json!("native"));
    session
        .start_turn("/ait-role-check", &spec.config)
        .await
        .unwrap();
    assert_native_roles(&complete(session.as_mut()).await, "ait-role-fixture/main");
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    session.close().await.unwrap();
    let mut reopened = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    reopened
        .start_turn("/ait-role-check", &spec.config)
        .await
        .unwrap();
    assert_native_roles(&complete(reopened.as_mut()).await, "ait-role-fixture/main");
    assert_eq!(
        reopened.persistence().unwrap().session_id,
        handle.session_id
    );
    reopened.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(agent.join("config.yml")).unwrap(),
        defaults
    );
    assert_eq!(
        std::fs::read_to_string(agent.join("models.yml")).unwrap(),
        models.to_string()
    );
}

fn assert_native_roles(events: &[AgentTurnEvent], advisor: &str) {
    let expected = json!({"default":"ait-role-fixture/fast","smol":"ait-role-fixture/fast",
        "vision":"ait-role-fixture/vision","advisor":advisor,"custom":"ait-role-fixture/vision"});
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry)
        if entry.item["text"].as_str().and_then(|text|serde_json::from_str::<Value>(text).ok()).as_ref() == Some(&expected))),"{events:?}");
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; preset discovery and management without inference"]
async fn installed_presets_resolve_aliases_and_manage_only_named_global_entries() {
    let root = tempfile::tempdir().unwrap();
    let agent = root.path().join("agent");
    std::fs::create_dir_all(agent.join("extensions")).unwrap();
    let (defaults, _) = native_profile(&agent, "http://127.0.0.1:9/v1");
    let global: Value = serde_json::from_str(&defaults).unwrap();
    std::fs::create_dir_all(root.path().join(".omp")).unwrap();
    let project = json!({"modelPresets":{"work":{"modelRoles":{
        "default":"@smol","smol":"ait-role-fixture/fast","vision":"ait-role-fixture/vision",
        "advisor":"ait-role-fixture/advisor:high","custom":"ait-role-fixture/vision"},
        "defaultThinkingLevel":"high"}}});
    std::fs::write(root.path().join(".omp/config.yml"), project.to_string()).unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(
            std::env::var_os("AIT_TEST_OMP_BIN")
                .expect("set AIT_TEST_OMP_BIN")
                .into(),
        )
    };
    let mut spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            feature_values: Some(BTreeMap::from([("modelPreset".to_owned(), json!("work"))])),
            ..Default::default()
        },
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    let presets = details
        .features
        .iter()
        .find(|feature| feature["id"] == "modelPreset")
        .unwrap();
    assert!(
        presets["options"][1]["metadata"]["error"].is_null(),
        "{presets}"
    );
    spec.config = client.resolve_selection(&spec).await.unwrap();
    assert_eq!(
        spec.config.model,
        Some(json!(["ait-role-fixture", "fast"]).to_string())
    );
    assert_eq!(spec.config.thinking_option_id.as_deref(), Some("high"));
    let mut owner = client.create_session(&spec).await.unwrap();
    owner
        .start_turn("/ait-role-check", &spec.config)
        .await
        .unwrap();
    assert_native_roles(&complete(owner.as_mut()).await, "ait-role-fixture/advisor");
    assert_eq!(
        std::fs::read_to_string(agent.join("config.yml")).unwrap(),
        defaults
    );
    let result = owner
        .begin_configured_action(
            "model_preset_save",
            &json!({"name":"created"}),
            &spec.config,
        )
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(result["error"].is_null(), "{result}");
    let saved = saved_settings(&agent);
    assert_eq!(saved["modelRoles"], global["modelRoles"]);
    assert_eq!(
        saved["modelPresets"]["created"]["modelRoles"]["smol"],
        "ait-role-fixture/fast"
    );
    assert!(
        saved["modelPresets"].get("work").is_none(),
        "project presets must not be copied globally"
    );
    let result = owner
        .begin_configured_action(
            "model_preset_delete",
            &json!({"name":"created"}),
            &spec.config,
        )
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(result["error"].is_null(), "{result}");
    let saved = saved_settings(&agent);
    assert!(saved["modelPresets"].get("created").is_none());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".omp/config.yml")).unwrap(),
        project.to_string()
    );
    owner.close().await.unwrap();
}

/// Write an isolated native model profile for official runtime acceptance tests.
pub(super) fn native_profile(agent: &std::path::Path, base_url: &str) -> (String, Value) {
    let entries = ["main","fast","vision","advisor"].map(|id|json!({"id":id,"name":id,"reasoning":true,
        "input":if id == "vision" {vec!["text","image"]} else {vec!["text"]},
        "contextWindow":32768,"maxTokens":1024,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}));
    let models = json!({"providers":{"ait-role-fixture":{
        "baseUrl":base_url,"api":"openai-completions","auth":"none",
        "models":entries}}});
    std::fs::write(agent.join("models.yml"), models.to_string()).unwrap();
    let defaults =
        json!({"modelRoles":{"default":"ait-role-fixture/main","smol":"ait-role-fixture/main",
        "vision":"ait-role-fixture/vision","advisor":"ait-role-fixture/main:high"},
        "modelTags":{"custom":{"name":"Custom helper"}}})
        .to_string();
    std::fs::write(agent.join("config.yml"), &defaults).unwrap();
    // A normal public extension resolves the same role aliases used by native advisor/task tools.
    std::fs::write(agent.join("extensions/role-check.ts"),r#"
export default function(pi) {
    pi.registerCommand("ait-role-check", {
        description: "Report native role resolution without model inference",
        handler: async (_args,ctx) => {
            const roles = Object.fromEntries(["default","smol","vision","advisor","custom"].map(role => {
                const model = ctx.models.resolve("@"+role);
                return [role,model ? model.provider+"/"+model.id : null];
            }));
            await pi.sendMessage({customType:"role-check",content:JSON.stringify(roles),display:true});
        }
    });
}

"#).unwrap();
    (defaults, models)
}

fn selected_roles() -> StoredAgentConfig {
    StoredAgentConfig {
        feature_values: Some(BTreeMap::from([
            (
                "modelRole.default".to_owned(),
                json!("ait-role-fixture/fast"),
            ),
            ("modelRole.smol".to_owned(), json!("ait-role-fixture/fast")),
            (
                "modelRole.advisor".to_owned(),
                json!("ait-role-fixture/advisor:high"),
            ),
            (
                "modelRole.custom".to_owned(),
                json!("ait-role-fixture/vision"),
            ),
        ])),
        ..StoredAgentConfig::default()
    }
}

/// Serve deterministic model responses without external inference or credentials.
pub(super) async fn model_stub() -> (String, tokio_util::task::AbortOnDropHandle<()>) {
    const COMPLETION: &str = r#"data: {"id":"roles","object":"chat.completion.chunk","created":0,"model":"fast","choices":[{"index":0,"delta":{"role":"assistant","content":"Role fixture ready"},"finish_reason":null}]}

data: {"id":"roles","object":"chat.completion.chunk","created":0,"model":"fast","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}

data: [DONE]

"#;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(async || ([("content-type", "text/event-stream")], COMPLETION)),
    );
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    (format!("http://{address}/v1"), server)
}

fn saved_settings(agent: &std::path::Path) -> Value {
    serde_yaml_ng::from_str(&std::fs::read_to_string(agent.join("config.yml")).unwrap()).unwrap()
}
