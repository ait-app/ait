use super::*;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; uses isolated plugins without paid inference"]
async fn installed_omp_confirmation_input_and_editor_complete_through_host_permissions() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    for method in ["confirm", "input", "editor"] {
        session
            .start_turn(&format!("/compat-dialog-{method}"), &fixture.spec.config)
            .await
            .unwrap();
        session.runtime_info().await.unwrap();
        let request = question(session.as_mut()).await;
        assert_eq!(request["title"], format!("OMP {method} fixture"));
        assert_eq!(request["kind"], "question");
        let answer = if method == "confirm" {
            assert_eq!(
                request["input"]["questions"][0]["question"],
                "OMP confirm fixture\n\nContinue this operation?"
            );
            "No"
        } else {
            "User answer"
        };
        let response = json!({"behavior":"allow","updatedInput":{"answers":{
            format!("OMP {method} fixture"):answer}}});
        session
            .respond_permission(request["id"].as_str().unwrap(), &response)
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        let expected = format!(
            "{method}: {}",
            if method == "confirm" {
                "false"
            } else {
                "User answer"
            }
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["detail"]["text"] == expected)),
            "{events:?}"
        );
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
        assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    }
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs native write/bash with a loopback mock model"]
async fn installed_omp_write_and_execution_wait_for_explicit_native_approval() {
    let mut fixture = installed_with_plugins().await;
    fixture.spec.config.mode_id = Some("ask".to_owned());
    for (prompt, approve) in [
        ("OMP_WRITE_APPROVAL_FIXTURE", false),
        ("OMP_EXEC_APPROVAL_FIXTURE", false),
        ("OMP_WRITE_APPROVAL_FIXTURE", true),
        ("OMP_EXEC_APPROVAL_FIXTURE", true),
    ] {
        let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
        session
            .start_turn(prompt, &fixture.spec.config)
            .await
            .unwrap();
        session.runtime_info().await.unwrap();
        let request = question(session.as_mut()).await;
        assert_eq!(session.pending_permissions(), vec![request.clone()]);
        // OMP 18.18.8 offers Approve, a session scope and Deny under a titled body.
        let (tool, detail) = if prompt == "OMP_WRITE_APPROVAL_FIXTURE" {
            (
                "write",
                json!({"type":"write","filePath":"approval-probe.html","content":"OMP_WRITE_OK"}),
            )
        } else {
            (
                "bash",
                json!({"type":"shell","command":"printf OMP_EXEC_OK"}),
            )
        };
        assert_eq!(request["kind"], "tool");
        assert_eq!(request["title"], format!("Allow tool: {tool}"));
        assert_eq!(request["detail"], detail);
        let labels: Vec<_> = request["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| action["label"].as_str().unwrap())
            .collect();
        assert_eq!(
            labels,
            [
                "Deny",
                "Approve",
                &format!("Approve {tool} for this session")
            ]
        );
        assert!(
            !std::path::Path::new(&fixture.spec.cwd)
                .join("approval-probe.html")
                .exists()
        );
        let response = json!({"behavior":if approve {"allow"} else {"deny"}});
        session
            .respond_permission(request["id"].as_str().unwrap(), &response)
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
        if prompt == "OMP_EXEC_APPROVAL_FIXTURE" && approve {
            assert!(
                matches!(events.last(), Some(AgentTurnEvent::Completed(Some(reply)))
                if reply.contains("OMP_EXEC_OK")),
                "{events:?}"
            );
        }
        if prompt == "OMP_WRITE_APPROVAL_FIXTURE" && approve {
            let path = std::path::Path::new(&fixture.spec.cwd).join("approval-probe.html");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "OMP_WRITE_OK");
            std::fs::remove_file(path).unwrap();
        }
        assert_eq!(session.pending_permissions(), Vec::<Value>::new());
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs native write with a loopback mock model"]
async fn installed_omp_cached_approval_offers_grants_and_keeps_the_written_content() {
    let mut fixture = installed_with_plugins().await;
    fixture.spec.config.mode_id = Some("ask-cached".to_owned());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("OMP_WRITE_APPROVAL_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();
    let request = question(session.as_mut()).await;

    // The folder listing OMP appends after the content is not part of it.
    assert_eq!(
        request["detail"],
        json!({"type":"write","filePath":"approval-probe.html","content":"OMP_WRITE_OK"})
    );
    let actions = request["actions"].as_array().unwrap();
    assert_eq!(actions[1]["label"], "Approve Once");
    assert_eq!(actions.len(), 6, "{actions:?}");
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow"}),
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let path = std::path::Path::new(&fixture.spec.cwd).join("approval-probe.html");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "OMP_WRITE_OK");
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; uses isolated plugins without paid inference"]
async fn installed_omp_structured_ask_dialog_answers_every_question_at_once() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("/compat-ask-dialog", &fixture.spec.config)
        .await
        .unwrap();
    let request = question(session.as_mut()).await;
    let shared: Value = serde_json::from_str(include_str!(
        "../../../../provider/tests/fixtures/omp_question.json"
    ))
    .unwrap();
    let questions = request["input"]["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[1]["multiSelect"], true);
    assert_eq!(questions[1]["answerFormat"], "array");

    session
        .respond_permission(request["id"].as_str().unwrap(), &shared["response"])
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    let expected = r#"ask: [["db",["A,B"],null],["features",["Auth","Search"],null]]"#;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["detail"]["text"] == expected)),
        "{events:?}"
    );
    session.close().await.unwrap();
}

async fn question(session: &mut dyn AgentSession) -> Value {
    loop {
        match next(session).await.unwrap() {
            AgentTurnEvent::PermissionRequested(request) => return request,
            AgentTurnEvent::Failed | AgentTurnEvent::Completed(_) => {
                panic!(
                    "OMP completed without publishing its dialog: {:?}",
                    session.failure_message()
                );
            }
            _ => {}
        }
    }
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; secret and empty answers stay verbatim without inference"]
async fn installed_omp_secret_and_empty_inputs_keep_native_value_semantics() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    for (command, answer, expected) in [
        (
            "/compat-dialog-secret",
            "  hunter2  ",
            "secret: \"  hunter2  \"",
        ),
        ("/compat-dialog-empty", "", "empty: \"\""),
    ] {
        session
            .start_turn(command, &fixture.spec.config)
            .await
            .unwrap();
        session.runtime_info().await.unwrap();
        let request = question(session.as_mut()).await;
        let field = &request["input"]["questions"][0];
        assert_eq!(field["allowEmpty"], true);
        assert_eq!(field["dismissBehavior"], "deny");
        assert_eq!(field["preserveWhitespace"], true);
        if command.ends_with("secret") {
            assert_eq!(field["secret"], true);
        } else {
            assert!(field.get("secret").is_none());
        }
        session
            .respond_permission(
                request["id"].as_str().unwrap(),
                &json!({"behavior":"allow","updatedInput":{"answers":{
                    field["header"].as_str().unwrap():answer}}}),
            )
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
                if entry.item["detail"]["text"] == expected)),
            "{events:?}"
        );
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
        assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    }
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; cancel returns native cancellation without inference"]
async fn installed_omp_dialog_cancel_returns_native_cancellation() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("/compat-dialog-input", &fixture.spec.config)
        .await
        .unwrap();
    session.runtime_info().await.unwrap();
    let request = question(session.as_mut()).await;
    session
        .respond_permission(request["id"].as_str().unwrap(), &json!({"behavior":"deny"}))
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    // Official OMP resolves a cancelled input as undefined (not null); the plugin sees
    // the cancellation and the turn still completes without a value.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["detail"]["text"] == "input: undefined")),
        "{events:?}"
    );
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; startup select answers before initialization completes"]
async fn installed_omp_startup_question_publishes_before_initialization_and_releases_one_input() {
    let mut fixture = installed_with_plugins().await;
    let mut values: BTreeMap<String, String> = fixture
        .client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    values.insert("AIT_OMP_STARTUP_DIALOG".to_owned(), "1".to_owned());
    fixture.client.environment = AgentEnvironment::try_from(values).unwrap();
    let mut session = tokio::time::timeout(
        Duration::from_secs(30),
        fixture.client.create_session(&fixture.spec),
    )
    .await
    .expect("startup dialog must not block session publication")
    .unwrap();
    let request = session
        .pending_permissions()
        .into_iter()
        .next()
        .expect("startup request before initialization");
    assert_eq!(request["title"], "OMP startup fixture");
    assert!(session.persistence().is_none());
    session
        .start_turn("Only after the startup answer", &fixture.spec.config)
        .await
        .unwrap();
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"Beta"}}}),
        )
        .await
        .unwrap();
    let events = tokio::time::timeout(Duration::from_secs(60), async {
        let mut events = Vec::new();
        loop {
            session.service_realtime().await.unwrap();
            while let Some(event) = session.poll_turn().unwrap() {
                let done = matches!(
                    event,
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Failed
                        | AgentTurnEvent::Cancelled
                );
                events.push(event);
                if done {
                    return events;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    // Official OMP admits prompts without a separate agent_start; the released input is
    // proven by its user message and completion, and the startup answer by the native
    // session file (custom plugin messages are not projected as text cards).
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::RuntimeInfo(info)
                if info.session_id.is_some())),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["text"] == "Only after the startup answer")),
        "{events:?}"
    );
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    assert!(session.persistence().is_some());
    let handle = session.persistence().unwrap();
    let file = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let journal = std::fs::read_to_string(file).unwrap();
    assert!(journal.contains("startup: \\\"Beta\\\""), "{journal}");
    session.close().await.unwrap();
}
