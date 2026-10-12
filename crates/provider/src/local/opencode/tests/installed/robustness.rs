//! Native regressions for protocol edge cases that otherwise surface as `AgentIo`.
use super::*;

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native migration of legacy provider/options/npm configuration"]
async fn installed_acp_legacy_provider_configuration_keeps_all_models_and_executes() {
    let mut fixture = installed(false).await;
    let path = fixture.root.path().join("opencode.json");
    let native: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut models = serde_json::Map::new();
    for index in 0..13 {
        models.insert(
            format!("legacy-{index:02}"),
            json!({
                "name":format!("Legacy {index:02}"),
                "limit":{"context":32000,"output":2000},
                "variants":{"brief":{"reasoningEffort":"low"},"deep":{"reasoningEffort":"high"}}
            }),
        );
    }
    let legacy = json!({"model":"local/legacy-12", "plugin":[], "provider":{"local":{
        "name":"Local legacy test", "npm":"@ai-sdk/openai-compatible",
        "options":native["providers"]["local"]["settings"], "models":models
    }}});
    std::fs::write(&path, legacy.to_string()).unwrap();
    let details = fixture.client.discover(&fixture.spec.cwd).await.unwrap();
    for index in 0..13 {
        let id = format!("local/legacy-{index:02}");
        let model = details
            .models
            .iter()
            .find(|model| model["id"] == id)
            .unwrap();
        assert!(
            model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|effort| effort["id"] == "deep")
        );
        assert_eq!(model["isDefault"], index == 12);
    }
    assert!(fixture.requests.try_recv().is_err());
    fixture.spec.config.model = Some("local/legacy-12".into());
    fixture.spec.config.thinking_option_id = Some("deep".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("legacy configured model", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy.to_string());
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; rejected creation cleanup and preserved resumed history"]
async fn installed_acp_rejected_initial_model_cleans_only_the_new_session() {
    let mut fixture = installed(false).await;
    let mut invalid = fixture.spec.clone();
    invalid.config.model = Some("local/not-installed".into());
    assert!(matches!(
        fixture.client.create_session(&invalid).await,
        Err(AgentSessionError::Rejected)
    ));
    let list = crate::ports::native_history::ListOptions {
        cwd: Some(fixture.spec.cwd.clone()),
        scan_limit: 100,
    };
    assert!(
        fixture
            .client
            .list_sessions(&list)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(fixture.requests.try_recv().is_err());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("keep native history", &fixture.spec.config)
        .await
        .unwrap();
    let history = completed(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert!(matches!(
        fixture
            .client
            .resume_session(&handle, &invalid, AgentResumePurpose::Interactive)
            .await,
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(fixture.client.list_sessions(&list).await.unwrap().len(), 1);
    assert_replay(
        &history,
        &fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .unwrap(),
    );
}

/// Answer deterministic approval scenarios; other native tests use the common model.
pub(super) fn model_response(request: &Value) -> Option<Response> {
    if is_user_message(request, "Approval sequence") {
        let has_result = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .take_while(|message| message["role"] != "user")
            .any(|message| message["role"] == "tool");
        return Some(if has_result {
            streamed_response(
                &json!({"role":"assistant","content":"authoritative answer"}),
                false,
            )
        } else {
            streamed_response(
                &json!({"role":"assistant","tool_calls":[{
                    "index":0,"id":uuid::Uuid::new_v4().to_string(),"type":"function",
                    "function":{"name":"shell","arguments":json!({"command":"pwd", "description":"Show directory"}).to_string()}
                }]}),
                true,
            )
        });
    }
    if !is_user_message(request, "Edit large approval file") {
        return None;
    }
    let results = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    let (name, arguments) = match results {
        0 => ("read", json!({"path":"large-approval.txt"})),
        1 => (
            "edit",
            json!({"path":"large-approval.txt", "oldString":"before sentinel", "newString":"after sentinel"}),
        ),
        _ => {
            return Some(streamed_response(
                &json!({"role":"assistant","content":"authoritative answer"}),
                false,
            ));
        }
    };
    Some(streamed_response(
        &json!({"role":"assistant","tool_calls":[{
            "index":0,"id":format!("large-{results}"),"type":"function",
            "function":{"name":name,"arguments":arguments.to_string()}
        }]}),
        true,
    ))
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; isolated provider snapshot and model refresh"]
async fn installed_acp_catalog_snapshot_refresh_keeps_configured_models_and_cleans_queries() {
    let mut fixture = installed(false).await;
    let path = fixture.root.path().join("opencode.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["providers"]["local"]["models"]["configured-model"] =
        json!({"name":"Configured model", "limit":{"context":32000,"output":2000}});
    config["model"] = json!("local/configured-model");
    std::fs::write(&path, config.to_string()).unwrap();
    let catalog = crate::service::provider_catalog::Catalog::default();
    let clients = BTreeMap::from([(
        "opencode".into(),
        Arc::new(fixture.client.clone()) as Arc<dyn AgentClient>,
    )]);
    let events = model::session::SessionEvents::default();
    let snapshot = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":fixture.spec.cwd}),
        )
        .await
        .unwrap();
    let entry = &snapshot["entries"][0];
    assert_eq!(entry["status"], "ready", "{entry}");
    assert!(
        entry["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "local/configured-model" && model["isDefault"] == true),
        "{entry}"
    );
    assert!(fixture.requests.try_recv().is_err());
    fixture.spec.config.model = Some("local/configured-model".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("configured snapshot model", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(
        session.runtime_info().await.unwrap().model.as_deref(),
        Some("local/configured-model")
    );
    session.close().await.unwrap();

    config["providers"]["local"]["models"]["refreshed-model"] =
        json!({"name":"Refreshed model", "limit":{"context":32000,"output":2000}});
    std::fs::write(&path, config.to_string()).unwrap();
    let acknowledgement = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.refresh.request",
            json!({"cwd":fixture.spec.cwd,"providers":["opencode"]}),
        )
        .await
        .unwrap();
    assert_eq!(acknowledgement["acknowledged"], true);
    let refreshed = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":fixture.spec.cwd}),
        )
        .await
        .unwrap();
    assert_eq!(refreshed["entries"][0]["status"], "ready");
    assert!(
        refreshed["entries"][0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "local/refreshed-model")
    );
    catalog.close().await;
    assert_eq!(
        fixture
            .client
            .list_sessions(&crate::ports::native_history::ListOptions {
                cwd: Some(fixture.spec.cwd.clone()),
                scan_limit: 100
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native once/always approval across multiple turns"]
async fn installed_acp_permission_once_and_always_keep_native_scope() {
    for action in ["once", "always"] {
        let fixture = installed(false).await;
        let mut spec = fixture.spec.clone();
        spec.config.feature_values = Some(BTreeMap::from([("permission".into(), json!("ask"))]));
        let mut session = fixture.client.create_session(&spec).await.unwrap();
        let mut approvals = 0;
        for _ in 0..2 {
            session
                .start_turn("Approval sequence", &spec.config)
                .await
                .unwrap();
            loop {
                match event(session.as_mut()).await {
                    AgentTurnEvent::PermissionRequested(request) => {
                        approvals += 1;
                        assert_eq!(request["kind"], "tool");
                        session
                            .respond_permission(
                                request["id"].as_str().unwrap(),
                                &json!({"behavior":"allow","selectedActionId":action}),
                            )
                            .await
                            .unwrap();
                    }
                    AgentTurnEvent::Completed(_) => break,
                    AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => {
                        panic!("approved turn failed")
                    }
                    _ => {}
                }
            }
            assert!(session.pending_permissions().is_empty());
        }
        assert_eq!(approvals, if action == "once" { 2 } else { 1 });
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; isolated native multiline shell approval"]
async fn installed_acp_multiline_shell_approval_preserves_input_and_continues() {
    let fixture = installed(true).await;
    let mut config = fixture.spec.config.clone();
    config.feature_values = Some(BTreeMap::from([("permission".into(), json!("ask"))]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Run shell multiline approval", &config)
        .await
        .unwrap();
    let request = loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => break request,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("multiline approval did not reach the client")
            }
            _ => {}
        }
    };
    let command = request["input"]["command"].as_str().unwrap();
    assert!(command.contains('\n'));
    assert!(command.len() > 1024);
    assert_eq!(request["title"], command);
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow"}),
        )
        .await
        .unwrap();
    let items = completed(session.as_mut()).await;
    let tool = items
        .iter()
        .find(|entry| entry.item["detail"]["type"] == "shell")
        .unwrap();
    assert_eq!(tool.item["status"], "completed");
    assert_eq!(
        tool.item["detail"]["output"].as_str().unwrap().trim(),
        fixture.spec.cwd
    );
    assert!(session.pending_permissions().is_empty());
    session
        .start_turn("after multiline approval", &config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native approval with a full-file diff over 64 KiB"]
async fn installed_acp_large_edit_preview_does_not_fail_tool_approval() {
    let fixture = installed(false).await;
    let path = fixture.root.path().join("large-approval.txt");
    let original = format!(
        "before sentinel\n{}",
        "unchanged file contents\n".repeat(5000)
    );
    std::fs::write(&path, &original).unwrap();
    let mut config = fixture.spec.config.clone();
    config.feature_values = Some(BTreeMap::from([("permission".into(), json!("ask"))]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Edit large approval file", &config)
        .await
        .unwrap();
    let mut edit_approved = false;
    let history = loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                if request["input"]["oldString"] == "before sentinel" {
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
                    edit_approved = true;
                }
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":"allow"}),
                    )
                    .await
                    .unwrap();
            }
            AgentTurnEvent::History(items) => break items,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("edit failed to settle")
            }
            _ => {}
        }
    };
    assert!(
        edit_approved,
        "tools: {:?}",
        history
            .iter()
            .filter(|entry| entry.item["type"] == "tool_call")
            .map(|entry| (&entry.item["status"], &entry.item["error"]))
            .collect::<Vec<_>>()
    );
    completed(session.as_mut()).await;
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original.replacen("before sentinel", "after sentinel", 1)
    );
    assert_eq!(history.iter().filter(|entry| entry.item["type"] == "tool_call" && entry.item["status"] == "completed").count(), 2);
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_replay(
        &history,
        &fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .unwrap(),
    );
}
