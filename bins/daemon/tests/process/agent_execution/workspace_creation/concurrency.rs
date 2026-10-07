//! The source creation e2e test keeps provisioning alive after its initiating socket closes.

use std::time::Duration;

use super::*;

#[tokio::test]
async fn observers_see_workspace_ready_before_native_start_and_concurrent_retries_join_one_creation()
 {
    let fixture = super::super::super::native::NativeFixture::new();
    std::fs::write(fixture.cwd.join("behavior"), "delayed-create").unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut observer = connect(&address, &methods()).await;
    call(
        &mut observer,
        "creation.subscribe.request",
        json!({"kind":"workspace","idempotencyKey":"in-flight"}),
    )
    .await;
    let params = json!({"idempotencyKey":"in-flight","workspaceId":"wks_0123456789abcdef","subscribe":true,
        "source":{"kind":"directory","path":fixture.cwd},
        "agent":{"agentId":"01234567-89ab-4def-8123-0123456789ab",
            "config":{"provider":"codex","cwd":fixture.cwd},"initialPrompt":"Create once"}});
    let mut creator = connect(&address, &methods()).await;
    creator
        .send(Message::Text(
            json!({"type":"request","request_id":"create",
        "method":"workspace.create.request","params":params})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let accepted = receive(&mut observer).await;
    assert_eq!(accepted["params"]["phase"], "accepted", "{accepted}");
    let ready = receive(&mut observer).await;
    assert_eq!(ready["params"]["phase"], "workspace_ready", "{ready}");
    assert_eq!(ready["params"]["workspaceId"], "wks_0123456789abcdef");
    assert_eq!(
        ready["params"]["agentId"],
        "01234567-89ab-4def-8123-0123456789ab"
    );
    for phase in ["accepted", "workspace_ready"] {
        let live = receive(&mut creator).await;
        assert_eq!(live["params"]["phase"], phase, "{live}");
    }
    let mut conflict = params.clone();
    conflict["idempotencyKey"] = json!("conflicting-agent-id");
    conflict["workspaceId"] = json!("wks_0123456789abcdee");
    let mut rejected = connect(&address, &methods()).await;
    assert_eq!(
        reply(&mut rejected, "workspace.create.request", conflict).await["code"],
        "idempotency_conflict"
    );
    assert_eq!(
        call(&mut rejected, "workspace.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut peer = connect(&address, &methods()).await;
    let retry = params.clone();
    let joined = tokio::spawn(async move {
        let result = call(&mut peer, "workspace.create.request", retry).await;
        peer.close(None).await.unwrap();
        result
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!joined.is_finished());
    creator.close(None).await.unwrap();
    std::fs::write(fixture.cwd.join("release-create"), "").unwrap();
    let result = joined.await.unwrap();
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(result["creation"]["phase"], "completed");
    assert_eq!(result["agent"]["id"], ready["params"]["agentId"]);
    let waited = call(
        &mut observer,
        "agent.finish.wait.request",
        json!({"agentId":result["agent"]["id"]}),
    )
    .await;
    assert_eq!(waited["lastMessage"], "Echo: Create once");
    let requests = std::fs::read_to_string(fixture.cwd.join("native-requests.jsonl")).unwrap();
    for method in ["thread/start", "turn/start"] {
        assert_eq!(
            requests
                .lines()
                .filter(|line| {
                    let request = serde_json::from_str::<Value>(line).unwrap();
                    request["method"] == method
                        && if method == "thread/start" {
                            request["params"]["ephemeral"] != true
                        } else {
                            request["params"]["threadId"]
                                == result["agent"]["persistence"]["sessionId"]
                        }
                })
                .count(),
            1
        );
    }
    terminate(&mut process).await;
}

#[tokio::test]
async fn creation_progress_is_released_with_the_operation_instead_of_leaking_connection_slots() {
    let fixture = super::super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    for (method, params) in [
        (
            "workspace.create.request",
            json!({"idempotencyKey":"plain-progress","subscribe":true,
            "source":{"kind":"directory","path":fixture.cwd}}),
        ),
        (
            "workspace.create.request",
            json!({"idempotencyKey":"composite-progress","subscribe":true,
            "source":{"kind":"directory","path":fixture.cwd},
            "agent":{"config":{"provider":"codex","cwd":fixture.cwd}}}),
        ),
        (
            "agent.create.request",
            json!({"idempotencyKey":"agent-progress","subscribe":true,
            "config":{"provider":"codex","cwd":fixture.cwd}}),
        ),
    ] {
        for _ in 0..20 {
            let result = call(&mut client, method, params.clone()).await;
            assert!(result["error"].is_null(), "{result}");
            assert_eq!(result["creation"]["phase"], "completed");
        }
    }
    terminate(&mut process).await;
}

#[tokio::test]
async fn attempted_initial_prompt_is_not_replayed_after_failure() {
    let fixture = super::super::super::native::NativeFixture::new();
    std::fs::write(fixture.cwd.join("behavior"), "reject-first-input").unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut capabilities = methods();
    capabilities.push("daemon.config.set.request");
    let mut client = connect(&address, &capabilities).await;
    // The fixture catalog intentionally has no small model; explicitly enable its
    // metadata model so this concurrency test still observes the auxiliary turn.
    call(
        &mut client,
        "daemon.config.set.request",
        json!({"config": {
            "metadataGeneration": {"providers": [{"provider": "codex", "model": "metadata-only"}]}
        }}),
    )
    .await;
    let params = json!({"idempotencyKey":"prompt-failure","source":{"kind":"directory","path":fixture.cwd},
        "agent":{"config":{"provider":"codex","cwd":fixture.cwd},"initialPrompt":"attempt once"}});
    let failed = call(&mut client, "workspace.create.request", params.clone()).await;
    assert_eq!(failed["creation"]["phase"], "failed", "{failed}");
    assert_eq!(failed["creation"]["outcomeUnknown"], true);
    assert!(failed["agent"]["id"].is_string());
    assert!(failed["error"].is_string());
    let repeated = call(&mut client, "workspace.create.request", params).await;
    assert_eq!(repeated["creation"], failed["creation"]);
    let session = failed["agent"]["persistence"]["sessionId"]
        .as_str()
        .unwrap();
    // Metadata generation has its own ephemeral thread in the same cwd and request log.
    // Observe that concurrent work so the assertion cannot depend on which task wins the race.
    let requests = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let log = std::fs::read_to_string(fixture.cwd.join("native-requests.jsonl")).unwrap();
            let requests: Vec<Value> = log
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if requests.iter().any(|request| {
                request["method"] == "turn/start"
                    && request["params"]["threadId"] != session
                    && request["params"]["outputSchema"].is_object()
            }) {
                break requests;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background title generation should attempt its own native turn");
    assert_eq!(
        requests
            .iter()
            .filter(|request| {
                request["method"] == "turn/start" && request["params"]["threadId"] == session
            })
            .count(),
        1,
        "the failed user input must not be replayed: {requests:?}"
    );
    terminate(&mut process).await;
}
