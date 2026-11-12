use super::*;

#[tokio::test]
async fn first_prompt_titles_reach_create_get_list_and_restart_without_overwriting_names() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = execution
        .execute(
            "agent.create.request",
            json!({
                "config":{"provider":"codex","cwd":fixture.cwd},
                "initialPrompt":"  修复\n session 标题  "
            }),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(created["agent"]["title"], "修复 session 标题");
    assert_eq!(created["creation"]["agent"]["title"], "修复 session 标题");
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    for method in [
        "agent.get.request",
        "agent.list.request",
        "agent.history.get.request",
    ] {
        let params = if method == "agent.get.request" {
            json!({"agentId":id})
        } else {
            json!({})
        };
        let result = execution.execute(method, params).await.unwrap();
        let agent = if method == "agent.get.request" {
            &result["agent"]
        } else {
            &result["entries"][0]["agent"]
        };
        assert_eq!(agent["title"], "修复 session 标题", "{method}");
    }
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"Follow up"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(
        registry.get(id).unwrap().unwrap().title.as_deref(),
        Some("修复 session 标题")
    );
    execution
        .execute(
            "agent.update.request",
            json!({"agentId":id,"name":"My title"}),
        )
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
    let (restarted, _) = worker(&fixture);
    let result = restarted
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(result["agent"]["title"], "My title");
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn deferred_first_prompt_and_local_history_recovery_fill_missing_titles() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = execution
        .execute(
            "agent.create.request",
            json!({
                "config":{"provider":"codex","cwd":fixture.cwd}
            }),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    assert!(created["agent"]["title"].is_null());
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"First question"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(
        registry.get(id).unwrap().unwrap().title.as_deref(),
        Some("First question")
    );
    execution.shutdown().await.unwrap();
    registry
        .update(id, &|current| {
            let mut next = current.clone();
            next.title = None;
            next
        })
        .unwrap();
    let before = registry.get(id).unwrap().unwrap();
    let native_calls = fixture.requests().len();
    let (restarted, reopened) = worker(&fixture);
    let result = restarted
        .execute("agent.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(result["entries"][0]["agent"]["title"], "First question");
    assert_eq!(fixture.requests().len(), native_calls);
    let recovered = reopened.get(id).unwrap().unwrap();
    assert_eq!(recovered.updated_at, before.updated_at);
    assert_eq!(recovered.last_user_message_at, before.last_user_message_at);
    restarted.shutdown().await.unwrap();

    // Older registrations may have no local projection until the native history is opened.
    crate::storage::timeline::Timeline::open(&fixture.root.path().join("timeline.sqlite"))
        .unwrap()
        .reconcile(id, "codex", &[])
        .unwrap();
    reopened
        .update(id, &|current| {
            let mut next = current.clone();
            next.title = None;
            next
        })
        .unwrap();
    let (restarted, reopened) = worker(&fixture);
    restarted
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(id).unwrap().unwrap().title.as_deref(),
        Some("First question")
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn imports_and_refreshes_use_native_titles_then_preview_then_user_history() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    for (native, metadata, expected) in [
        (
            "named",
            json!({"name":" Native title ","preview":"Preview"}),
            "Native title",
        ),
        (
            "preview",
            json!({"name":" ","preview":" Preview title "}),
            "Preview title",
        ),
        ("history", json!({"preview":" "}), "Historical question"),
    ] {
        std::fs::write(
            fixture.cwd.join(format!("native-session-{native}.json")),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        if native == "history" {
            std::fs::write(
                fixture.cwd.join(format!("native-history-{native}.json")),
                serde_json::to_vec(&json!([
                    {"id":"turn","status":"completed","items":[{"id":"user","type":"userMessage",
                        "content":[{"type":"text","text":"Historical question"}]}]}
                ]))
                .unwrap(),
            )
            .unwrap();
        }
        let imported = execution
            .execute(
                "agent.import.request",
                json!({
                    "providerId":"codex","providerHandleId":native,"cwd":fixture.cwd
                }),
            )
            .await
            .unwrap();
        let id = imported["agentId"].as_str().unwrap();
        assert_eq!(imported["agent"]["title"], expected);
        registry
            .update(id, &|current| {
                let mut next = current.clone();
                next.title = None;
                next
            })
            .unwrap();
        let refreshed = execution
            .execute("agent.refresh.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(refreshed["agent"]["title"], expected);
        execution
            .execute(
                "agent.update.request",
                json!({"agentId":id,"name":"Keep my title"}),
            )
            .await
            .unwrap();
        let refreshed = execution
            .execute("agent.refresh.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(refreshed["agent"]["title"], "Keep my title");
    }
    execution.shutdown().await.unwrap();
}
