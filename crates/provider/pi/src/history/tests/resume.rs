use super::*;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use std::os::unix::fs::{PermissionsExt, symlink};

use provider::service::agent_execution::AgentExecution;

fn worker(
    root: &Path,
    cwd: &str,
    client: PiClient,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    provider::testing::worker_at(
        root,
        Path::new(cwd),
        model::creation::Creations::default(),
        Box::new(client),
    )
}

#[tokio::test]
async fn unknown_resume_preserves_symlink_spelling_from_metadata_and_overrides() {
    for override_cwd in [false, true] {
        let (directory, mut client, _file, _cwd) = fixture();
        let project = directory.path().join("project");
        let linked = directory.path().join("linked-project");
        std::fs::create_dir(&project).unwrap();
        symlink(&project, &linked).unwrap();
        let cwd = project.canonicalize().unwrap().to_str().unwrap().to_owned();
        let folder = project_directory(&directory.path().join("sessions"), &linked).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("session.jsonl");
        write(
            &file,
            &cwd,
            vec![
                json!({"type":"thinking_level_change","id":"thinking","parentId":null,"thinkingLevel":"off"}),
                user("u", json!("thinking"), "linked"),
                assistant("a", "u", json!([{"type":"text","text":"done"}])),
            ],
        );
        let program = directory.path().join("pi");
        std::fs::write(&program, include_str!("../../tests/fixtures/pi.py")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        client.program = program;
        let (execution, registry) = worker(directory.path(), &cwd, client.clone());
        let params = if override_cwd {
            json!({"handle":{"provider":"pi","sessionId":SESSION,"metadata":{"cwd":cwd}},"overrides":{"cwd":linked}})
        } else {
            json!({"handle":{"provider":"pi","sessionId":SESSION,"metadata":{"cwd":linked}}})
        };
        let response = execution
            .execute("agent.resume.request", params)
            .await
            .unwrap();
        let id = response["agentId"].as_str().unwrap();
        let record = registry.get(id).unwrap().unwrap();
        assert_eq!(record.cwd, cwd);
        assert_eq!(
            record
                .config
                .as_ref()
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("off")
        );
        assert_eq!(
            record
                .runtime_info
                .as_ref()
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("off")
        );
        assert_eq!(
            record
                .persistence
                .as_ref()
                .unwrap()
                .metadata
                .as_ref()
                .unwrap()["nativeCwd"],
            json!(linked)
        );
        execution.shutdown().await.unwrap();
        let renamed = folder.join("renamed.jsonl");
        std::fs::rename(file, &renamed).unwrap();
        let (restarted, _) = worker(directory.path(), &cwd, client);
        restarted
            .execute("agent.resume.request", json!({"handle":record.persistence}))
            .await
            .unwrap();
        restarted
            .execute("agent.refresh.request", json!({"agentId":id}))
            .await
            .unwrap();
        restarted.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn observed_resume_recovers_renamed_history_by_session_id() {
    for native_path in [false, true] {
        let (directory, mut client, file, cwd) = fixture();
        write(
            &file,
            &cwd,
            vec![
                user("u", Value::Null, "saved work"),
                assistant("a", "u", json!([{"type":"text","text":"done"}])),
                json!({"type":"thinking_level_change","id":"thinking","parentId":"a",
                    "thinkingLevel":"low"}),
            ],
        );
        let mut saved = handle();
        saved.metadata = Some(BTreeMap::from([
            ("cwd".into(), json!(cwd)),
            ("nativeCwd".into(), json!(cwd)),
            ("hasInput".into(), json!(false)),
            ("observedWork".into(), json!(true)),
            ("sessionFile".into(), json!(file)),
        ]));
        if native_path {
            saved.native_handle = Some(json!(file));
        }
        let renamed = file.with_file_name("rotated.jsonl");
        std::fs::rename(&file, &renamed).unwrap();
        let saved_history = std::fs::read_to_string(&renamed).unwrap();
        let program = directory.path().join("pi");
        std::fs::write(&program, include_str!("../../tests/fixtures/pi.py")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        client.program = program;
        let spec = AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd,
            config: StoredAgentConfig::default(),
            confinement: None,
        };

        let mut resumed = client
            .resume_session(&saved, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();

        let arguments: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
        )
        .unwrap();
        let session_arg = arguments
            .iter()
            .position(|argument| argument == "--session")
            .expect("observed work was reopened without its renamed native history");
        assert_eq!(
            arguments[session_arg + 1],
            renamed.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(
            resumed
                .runtime_info()
                .await
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("low")
        );
        let metadata = resumed.persistence().unwrap().metadata.unwrap();
        assert_eq!(metadata["hasInput"], true);
        assert!(!metadata.contains_key("observedWork"));
        resumed.close().await.unwrap();
        assert!(!file.exists());
        assert_eq!(std::fs::read_to_string(&renamed).unwrap(), saved_history);

        // Conflicting identities must fail instead of silently opening a new session.
        std::fs::copy(&renamed, renamed.with_file_name("duplicate.jsonl")).unwrap();
        assert!(matches!(
            client
                .resume_session(&saved, &spec, AgentResumePurpose::Interactive)
                .await,
            Err(AgentSessionError::Rejected)
        ));
    }
}

#[tokio::test]
async fn observed_resume_without_saved_history_keeps_the_weaker_handle() {
    let (directory, mut client, file, cwd) = fixture();
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("../../tests/fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: cwd.clone(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    let mut saved = handle();
    saved.native_handle = Some(json!(file));
    saved.metadata = Some(BTreeMap::from([
        ("cwd".into(), json!(cwd)),
        ("hasInput".into(), json!(false)),
        ("observedWork".into(), json!(true)),
    ]));

    let mut resumed = client
        .resume_session(&saved, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();

    let metadata = resumed.persistence().unwrap().metadata.unwrap();
    assert_eq!(metadata["hasInput"], false);
    assert_eq!(metadata["observedWork"], true);
    resumed.close().await.unwrap();
    assert!(!file.exists());
    // A required file remains mandatory even if observed-work evidence is also present.
    saved
        .metadata
        .as_mut()
        .unwrap()
        .insert("hasInput".into(), json!(true));
    assert!(matches!(
        client
            .resume_session(&saved, &spec, AgentResumePurpose::Interactive)
            .await,
        Err(AgentSessionError::Unavailable)
    ));
}

#[tokio::test]
async fn imported_thinking_from_an_earlier_model_is_clamped_before_the_first_prompt() {
    let (directory, mut client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "initial"),
            assistant("a", "u", json!([{"type":"text","text":"done"}])),
            json!({"type":"thinking_level_change","id":"thinking","parentId":"a",
                "thinkingLevel":"xhigh"}),
            json!({"type":"model_change","id":"model","parentId":"thinking",
                "provider":"fixture","modelId":"low-ceiling"}),
        ],
    );
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("../../tests/fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    client.environment =
        provider_core::ports::environment::AgentEnvironment::try_from(BTreeMap::from([(
            "PI_FIXTURE_SCENARIO".into(),
            "strict-thinking-models".into(),
        )]))
        .unwrap();
    let (execution, registry) = worker(directory.path(), &cwd, client);

    let resumed = execution
        .execute(
            "agent.resume.request",
            json!({"handle":{"provider":"pi","sessionId":SESSION,"metadata":{"cwd":cwd}}}),
        )
        .await
        .unwrap();

    let id = resumed["agentId"].as_str().unwrap();
    let record = registry.get(id).unwrap().unwrap();
    let config = record.config.unwrap();
    assert_eq!(config.model.as_deref(), Some("fixture/low-ceiling"));
    assert_eq!(config.thinking_option_id.as_deref(), Some("xhigh"));
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("low")
    );
    let sent = execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"after import","messageId":"after-import"}),
        )
        .await
        .unwrap();
    assert_eq!(sent["accepted"], true);
    execution
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    assert!(registry.get(id).unwrap().unwrap().last_error.is_none());
    execution.shutdown().await.unwrap();
}
