use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses isolated configuration without model inference"]
async fn installed_omp_discovers_and_opens_native_rpc_without_inference() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                root.path().join("agent").to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into()).with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            mode_id: Some("ask".to_owned()),
            ..StoredAgentConfig::default()
        },
    };
    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.modes.len(), 4);
    client.commands(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.provider, "omp");
    assert_eq!(info.mode_id.as_deref(), Some("ask"));
    assert!(info.session_id.is_some());
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 0);
    let mut reopened = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    reopened.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; probes real native model capabilities without inference"]
async fn installed_omp_tiers_and_thinking_follow_model_without_changing_defaults() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let native_models = ["gpt-5.4", "deepseek-v4.1-flash"].map(|id| {
        json!({
            "id":id,"name":id,"reasoning":true,"input":["text"],
            "thinking":{"mode":"effort","efforts":["low","high","max"]},
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}})
    });
    let models = json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":native_models}}});
    std::fs::write(agent.join("models.yml"), models.to_string()).unwrap();
    let defaults = "modelRoles:\n  default: ait-local-fixture/deepseek-v4.1-flash:max\n";
    std::fs::write(agent.join("config.yml"), defaults).unwrap();
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
        config: StoredAgentConfig::default(),
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.features.len(), 2);
    for id in ["gpt-5.4", "deepseek-v4.1-flash"] {
        let route = json!(["ait-local-fixture", id]).to_string();
        let model = details
            .models
            .iter()
            .find(|model| model["id"] == route)
            .unwrap();
        assert!(
            model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|level| level["id"] == "max")
        );
        spec.config.model = Some(route);
        let features = client.draft_features(&spec).await.unwrap();
        assert_eq!(
            features.iter().any(|feature| feature["id"] == "fastMode"),
            id == "gpt-5.4"
        );
        assert_eq!(
            features.iter().any(|feature| feature["id"] == "slowMode"),
            id == "gpt-5.4"
        );
    }
    assert_eq!(
        std::fs::read_to_string(agent.join("config.yml")).unwrap(),
        defaults
    );
    assert_eq!(
        std::fs::read_to_string(agent.join("models.yml")).unwrap(),
        models.to_string()
    );
    assert!(!agent.join("sessions").exists());
}
