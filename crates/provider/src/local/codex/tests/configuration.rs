use super::*;

#[tokio::test]
async fn auto_review_cua_approval_is_native_and_is_cleared_on_the_same_thread() {
    let fixture = Fixture::new();
    fixture.mode("workflows");
    let client = fixture.client();
    let mut spec = fixture.spec();
    spec.config.mode_id = Some("auto".to_owned());
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence();
    session.start_turn("first", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    spec.config.mode_id = Some("auto-review".to_owned());
    session.start_turn("second", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    spec.config.mode_id = Some("auto".to_owned());
    session.start_turn("third", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    assert_eq!(session.persistence(), handle);
    session.close().await.unwrap();
    let requests = fixture.requests();
    let start = &requests
        .iter()
        .find(|request| request["method"] == "thread/start")
        .unwrap()["params"];
    assert_eq!(start["approvalPolicy"], "on-request");
    assert_eq!(start["sandbox"], "workspace-write");
    assert_eq!(start["approvalsReviewer"], "user");
    assert_eq!(start["config"], json!({}));
    let resumed: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "thread/resume")
        .map(|request| &request["params"])
        .collect();
    assert_eq!(resumed.len(), 2);
    assert_eq!(resumed[0]["approvalPolicy"], "on-request");
    assert_eq!(resumed[0]["sandbox"], "workspace-write");
    assert_eq!(resumed[0]["approvalsReviewer"], "auto_review");
    assert_eq!(
        resumed[0]["config"]["plugins"]["unified-computer-use@openai-bundled"]["mcp_servers"]["cua_repl"]
            ["tools"]["js"]["approval_mode"],
        "approve"
    );
    assert_eq!(resumed[1]["approvalsReviewer"], "user");
    assert_eq!(resumed[1]["config"], json!({}));
}

#[tokio::test]
async fn computer_use_approval_choices_preserve_native_labels_in_auto_review() {
    for answer in ["Accept", "Decline", "Cancel"] {
        let fixture = Fixture::new();
        fixture.mode("workflows");
        let client = fixture.client();
        let mut spec = fixture.spec();
        spec.config.mode_id = Some("auto-review".to_owned());
        let mut session = client.create_session(&spec).await.unwrap();
        session
            .start_turn("permit-cua", &spec.config)
            .await
            .unwrap();
        let AgentTurnEvent::PermissionRequested(request) =
            terminal(session.as_mut()).await.unwrap()
        else {
            panic!("expected a native computer use approval");
        };
        assert_eq!(request["kind"], "question");
        assert_eq!(request["input"]["questions"][0]["header"], "cua_repl.js");
        session
            .respond_permission(
                request["id"].as_str().unwrap(),
                &json!({"behavior":"allow","updatedInput":{"answers":{"cua-approval":answer}}}),
            )
            .await
            .unwrap();
        assert!(session.pending_permissions().is_empty());
        assert!(matches!(
            terminal(session.as_mut()).await.unwrap(),
            AgentTurnEvent::Completed(_)
        ));
        let requests = fixture.requests();
        let reply = requests
            .iter()
            .find(|request| request.get("method").is_none() && request["id"] == "approval-1")
            .unwrap();
        assert_eq!(
            reply["result"],
            json!({"answers":{"cua-approval":{"answers":[answer]}}})
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn advanced_options_override_presets_and_reconfigure_the_same_native_thread() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut spec = fixture.spec();
    spec.config = serde_json::from_value(json!({"modeId":"read-only", "providerOptions":{
        "approval_policy":{"granular":{"sandbox_approval":true,"mcp_elicitations":true}},
        "sandbox_mode":"workspace-write", "sandbox_workspace_write":{
            "writable_roots":["/tmp/fixture"],"network_access":true,"exclude_slash_tmp":true}},
        "mcpServers":{"docs":{"type":"http","url":"https://example.com/mcp"}},
        "toolPolicy":{"preapproved":[{"kind":"mcp","server":"docs","tool":"search"}]}}))
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence();
    session
        .start_turn("configured", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    spec.config.mcp_servers = None;
    spec.config.provider_options = None;
    spec.config.tool_policy = None;
    session.start_turn("cleared", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    assert_eq!(session.persistence(), handle);
    session.close().await.unwrap();
    let requests = fixture.requests();
    let start = &requests
        .iter()
        .find(|request| request["method"] == "thread/start")
        .unwrap()["params"];
    assert_eq!(
        start["approvalPolicy"]["granular"]["sandbox_approval"],
        true
    );
    assert_eq!(start["sandbox"], "workspace-write");
    assert_eq!(
        start["config"]["mcp_servers"]["docs"]["tools"]["search"]["approval_mode"],
        "approve"
    );
    let turns: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "turn/start")
        .collect();
    assert_eq!(
        turns[0]["params"]["sandboxPolicy"],
        json!({"type":"workspaceWrite","networkAccess":true,"excludeSlashTmp":true,"writableRoots":["/tmp/fixture"]})
    );
    assert_eq!(turns[1]["params"]["sandboxPolicy"]["type"], "readOnly");
    let resumed = &requests
        .iter()
        .find(|request| request["method"] == "thread/resume")
        .unwrap()["params"];
    assert_eq!(resumed["config"], json!({}));
    assert_eq!(resumed["approvalPolicy"], "never");
}

#[tokio::test]
async fn thread_usage_notifications_without_turn_identity_are_retained() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("usage", &spec.config).await.unwrap();
    let AgentTurnEvent::Usage(usage) = terminal(session.as_mut()).await.unwrap() else {
        panic!("expected native usage before completion");
    };
    assert_eq!(usage.context_window_used_tokens, Some(107));
    assert_eq!(usage.cached_input_tokens, Some(30));
    assert_eq!(usage.context_window_max_tokens, Some(200_000));
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    session.close().await.unwrap();
}
