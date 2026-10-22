use super::*;

#[tokio::test]
#[ignore = "requires explicit AIT_TEST_OMP_REAL_* configuration; performs paid model inference"]
async fn installed_omp_real_inference_tools_questions_and_resume() {
    let (_root, client, spec) = real_fixture();
    let work = PathBuf::from(&spec.cwd);
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(
        details
            .models
            .iter()
            .any(|item| item["id"] == *spec.config.model.as_ref().unwrap())
    );
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        spec.config.model
    );
    let token = format!("AIT_OMP_{}", uuid::Uuid::new_v4().simple());
    session
        .start_turn(
            &format!("Remember this integration token: {token}. Reply with exactly this token. Do not call tools."),
            &spec.config,
        )
        .await
        .unwrap();
    let first = drain_real(session.as_mut()).await;
    assert_completed_with(&first, &token);
    assert!(
        first
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Progress { .. }))
    );
    assert!(first.iter().any(|event| matches!(event, AgentTurnEvent::Usage(usage) if usage.output_tokens.is_some_and(|tokens| tokens > 0))));
    eprintln!("real OMP: streaming text and token usage passed");
    session.start_turn(
        "Integration test in this temporary workspace only: use the write tool to create probe.txt with exactly OMP_TOOL_OK. Then use the ask tool to ask one question with id choice, text Pick a test value, and two options Alpha and Beta. After receiving the answer, reply with OMP_INTERACTION_OK and the chosen label. Do not run shell commands, access other files, or use the network.",
        &spec.config,
    ).await.unwrap();
    let second = drain_real(session.as_mut()).await;
    assert_completed_with(&second, "OMP_INTERACTION_OK");
    assert_completed_with(&second, "Alpha");
    assert_eq!(
        std::fs::read_to_string(work.join("probe.txt"))
            .unwrap()
            .trim(),
        "OMP_TOOL_OK"
    );
    assert!(second.iter().any(|event| matches!(event, AgentTurnEvent::PermissionRequested(request) if request["input"]["questions"][0]["answerFormat"] == "array")));
    assert!(second.iter().any(|event| matches!(event, AgentTurnEvent::PermissionRequested(request) if request["input"]["questions"][0]["options"].as_array().is_some_and(|options| options.iter().any(|option|option["label"] == "Approve")))));
    assert!(second.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" && entry.item["status"] == "completed")));
    eprintln!("real OMP: native tool, approval and structured ask passed");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let live_keys: Vec<_> = first
        .iter()
        .chain(&second)
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.key.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        live_keys,
        history
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        client
            .list_sessions(&ListOptions {
                cwd: Some(spec.cwd.clone()),
                scan_limit: 100
            })
            .await
            .unwrap()
            .len(),
        1
    );
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.start_turn("Without reading files or calling tools, reply with the integration token from the first user message. Output only that token.", &spec.config).await.unwrap();
    let third = drain_real(resumed.as_mut()).await;
    assert_completed_with(&third, &token);
    resumed.close().await.unwrap();
    eprintln!("real OMP: native history, session listing and resumed model context passed");
}

fn real_fixture() -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let agent = root.path().join("agent");
    let work = root.path().join("work");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let model = std::env::var("AIT_TEST_OMP_REAL_MODEL").expect("set AIT_TEST_OMP_REAL_MODEL");
    let base_url = std::env::var("AIT_TEST_OMP_REAL_BASE_URL").expect("set real endpoint");
    assert!(std::env::var("AIT_TEST_OMP_REAL_API_KEY").is_ok());
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-integration":{
            "baseUrl":base_url,"api":"openai-completions",
            "apiKey":"AIT_TEST_OMP_REAL_API_KEY",
            "compat":{"supportsStore":false,"supportsDeveloperRole":false,
                "maxTokensField":"max_tokens","supportsStrictMode":false},
            "models":[{"id":model,"name":model,"reasoning":false,"input":["text"],
                "contextWindow":128_000,"maxTokens":4096,
                "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            ("HOME".to_owned(), root.path().to_str().unwrap().to_owned()),
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(std::env::var_os("AIT_TEST_OMP_BIN").unwrap().into())
            .with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: work.to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            model: Some(json!(["ait-integration", model]).to_string()),
            mode_id: Some("ask".to_owned()),
            feature_values: Some(BTreeMap::from([("autoRetry".to_owned(), json!(false))])),
            ..StoredAgentConfig::default()
        },
    };
    (root, client, spec)
}

async fn drain_real(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    tokio::time::timeout(Duration::from_secs(240), async {
        let mut events = Vec::new();
        loop {
            let Some(event) = session.poll_turn().unwrap() else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            };
            if let AgentTurnEvent::PermissionRequested(request) = &event {
                let mut answers = serde_json::Map::new();
                for question in request["input"]["questions"].as_array().unwrap() {
                    let options = question["options"].as_array().unwrap();
                    let label = options
                        .iter()
                        .filter_map(|option| option["label"].as_str())
                        .find(|label| matches!(*label, "Approve" | "Alpha"))
                        .expect("only approve the test tool or choose Alpha");
                    answers.insert(
                        question["answerKey"].as_str().unwrap().to_owned(),
                        if question["answerFormat"] == "array" {
                            json!([label])
                        } else {
                            json!(label)
                        },
                    );
                }
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":"allow","updatedInput":{"answers":answers}}),
                    )
                    .await
                    .unwrap();
            }
            if matches!(event, AgentTurnEvent::Failed) {
                report_native_failure(session);
            }
            let done = matches!(
                event,
                AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed | AgentTurnEvent::Cancelled
            );
            events.push(event);
            if done {
                return events;
            }
        }
    })
    .await
    .expect("real OMP inference timed out")
}

fn assert_completed_with(events: &[AgentTurnEvent], expected: &str) {
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text))) if text.contains(expected)),
        "real OMP did not complete with expected text: {events:?}"
    );
}

fn report_native_failure(session: &dyn AgentSession) {
    let handle = session.persistence().unwrap();
    let path = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let secret = std::env::var("AIT_TEST_OMP_REAL_API_KEY").unwrap();
    for line in text.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        if let Some(error) = record["message"]["errorMessage"].as_str() {
            let safe = if secret.is_empty() {
                error.to_owned()
            } else {
                error.replace(&secret, "[REDACTED]")
            };
            eprintln!("real OMP native failure: {safe}");
        }
    }
}
