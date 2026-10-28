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
        let response = if method == "confirm" {
            assert_eq!(request["kind"], "tool");
            json!({"behavior":"allow"})
        } else {
            assert_eq!(request["kind"], "question");
            json!({"behavior":"allow","updatedInput":{"answers":{
                format!("OMP {method} fixture"):"User answer"}}})
        };
        session
            .respond_permission(request["id"].as_str().unwrap(), &response)
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        let expected = format!(
            "{method}: {}",
            if method == "confirm" {
                "true"
            } else {
                "User answer"
            }
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["text"] == expected)),
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
        assert_eq!(request["kind"], "question");
        assert_eq!(session.pending_permissions(), vec![request.clone()]);
        let question = &request["input"]["questions"][0];
        assert!(
            question["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["label"] == "Approve")
        );
        assert!(
            !std::path::Path::new(&fixture.spec.cwd)
                .join("approval-probe.html")
                .exists()
        );
        let response = if approve {
            json!({"behavior":"allow","updatedInput":{"answers":{
                question["header"].as_str().unwrap():"Approve"}}})
        } else {
            json!({"behavior":"deny"})
        };
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
