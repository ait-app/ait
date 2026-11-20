//! Resource claims precede Workspace provisioning, including records without old receipts.

use super::*;

#[tokio::test]
async fn completed_workspace_receipts_reject_missing_directories_until_restored() {
    let fixture = super::super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    let params = json!({"idempotencyKey":"missing-directory",
        "source":{"kind":"directory","path":fixture.cwd}});
    let first = call(&mut client, "workspace.create.request", params.clone()).await;
    let moved = fixture.root.path().join("temporarily-moved");
    std::fs::rename(&fixture.cwd, &moved).unwrap();
    for (method, request) in [
        ("workspace.create.request", params.clone()),
        (
            "creation.subscribe.request",
            json!({"kind":"workspace","idempotencyKey":"missing-directory","subscribe":false}),
        ),
    ] {
        assert_eq!(
            reply(&mut client, method, request).await["code"],
            "workspace_not_found"
        );
    }
    std::fs::rename(moved, &fixture.cwd).unwrap();
    let restored = call(&mut client, "workspace.create.request", params).await;
    assert_eq!(restored["creation"], first["creation"]);
    terminate(&mut process).await;
}

#[tokio::test]
async fn completed_receipts_never_report_deleted_agents_or_recreate_them() {
    let fixture = super::super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    let requests = [
        (
            "agent",
            "agent.create.request",
            json!({"idempotencyKey":"deleted-agent",
            "config":{"provider":"codex","cwd":fixture.cwd}}),
        ),
        (
            "workspace",
            "workspace.create.request",
            json!({"idempotencyKey":"deleted-agent",
            "source":{"kind":"directory","path":fixture.cwd},
            "agent":{"config":{"provider":"codex","cwd":fixture.cwd}}}),
        ),
    ];
    for (_, method, params) in &requests {
        assert_eq!(
            call(&mut client, method, params.clone()).await["creation"]["phase"],
            "completed"
        );
    }
    client.close(None).await.unwrap();
    terminate(&mut process).await;
    std::fs::write(state.join("agents/agents.json"), "[]").unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    for (kind, method, params) in requests {
        assert_eq!(
            reply(&mut client, method, params).await["code"],
            "agent_not_found"
        );
        assert_eq!(
            reply(
                &mut client,
                "creation.subscribe.request",
                json!({
            "kind":kind,"idempotencyKey":"deleted-agent","subscribe":false})
            )
            .await["code"],
            "agent_not_found"
        );
    }
    assert_eq!(
        call(&mut client, "agent.list.request", json!({})).await["entries"],
        json!([])
    );
    terminate(&mut process).await;
}

#[tokio::test]
async fn existing_agent_without_creation_receipt_cannot_provision_another_workspace() {
    let fixture = super::super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    let created = call(
        &mut client,
        "agent.create.request",
        json!({
        "config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await;
    let id = created["agentId"].clone();
    assert!(id.is_string(), "{created}");
    client.close(None).await.unwrap();
    terminate(&mut process).await;
    // Model a registered/imported Agent whose creation predates durable receipts.
    std::fs::remove_file(state.join("creations/receipts.sqlite3")).unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    for (method, params) in [
        (
            "workspace.create.request",
            json!({"idempotencyKey":"existing-composite",
            "source":{"kind":"directory","path":fixture.cwd},
            "agent":{"agentId":id,"config":{"provider":"codex","cwd":fixture.cwd}}}),
        ),
        (
            "agent.create.request",
            json!({"idempotencyKey":"existing-agent","agentId":id,
            "config":{"provider":"codex","cwd":fixture.cwd}}),
        ),
    ] {
        assert_eq!(
            reply(&mut client, method, params).await["code"],
            "idempotency_conflict"
        );
    }
    assert_eq!(
        call(&mut client, "workspace.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&mut client, "agent.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    terminate(&mut process).await;
}
