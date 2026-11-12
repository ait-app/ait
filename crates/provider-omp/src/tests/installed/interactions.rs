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
    let shared: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/omp_question.json")).unwrap();
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
