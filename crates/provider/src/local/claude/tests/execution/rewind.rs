use super::*;

#[tokio::test]
async fn native_file_and_combined_rewind_preserve_the_source_conversation() {
    let fixture = Fixture::new();
    let execution = worker(&fixture);
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":"claude","cwd":fixture.cwd,"modeId":"default"}}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    let original = created["agent"]["persistence"]["sessionId"].clone();
    for text in ["first", "second"] {
        assert_eq!(
            request(
                &execution,
                "agent.message.send.request",
                json!({"agentId":id,"text":text,"messageId":text})
            )
            .await["accepted"],
            true
        );
        request(
            &execution,
            "agent.finish.wait.request",
            json!({"agentId":id}),
        )
        .await;
    }
    let page = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    let target = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["item"]["clientMessageId"] == "second")
        .unwrap()["item"]["messageId"]
        .clone();
    assert_rewind(&execution, &fixture, &id, &target, &original).await;
    execution.shutdown().await.unwrap();
}

async fn assert_rewind(
    execution: &AgentExecution,
    fixture: &Fixture,
    id: &str,
    target: &Value,
    original: &Value,
) {
    std::fs::write(fixture.cwd.join("tracked.txt"), "changed").unwrap();
    assert_eq!(
        request(
            execution,
            "agent.rewind.request",
            json!({"agentId":id,"messageId":target,"mode":"files"})
        )
        .await["ok"],
        true
    );
    assert_eq!(
        std::fs::read_to_string(fixture.cwd.join("tracked.txt")).unwrap(),
        "checkpoint restored"
    );
    let snapshot = request(execution, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(
        snapshot["agent"]["runtimeInfo"]["sessionId"], *original,
        "{snapshot}"
    );
    std::fs::write(fixture.cwd.join("tracked.txt"), "changed again").unwrap();
    assert_eq!(
        request(
            execution,
            "agent.rewind.request",
            json!({"agentId":id,"messageId":target,"mode":"both"})
        )
        .await["ok"],
        true
    );
    assert_eq!(
        std::fs::read_to_string(fixture.cwd.join("tracked.txt")).unwrap(),
        "checkpoint restored"
    );
    let snapshot = request(execution, "agent.get.request", json!({"agentId":id})).await;
    assert_ne!(snapshot["agent"]["runtimeInfo"]["sessionId"], *original);
    let page = request(
        execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(
        !page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["item"]["text"] == "second")
    );
    let sent = request(
        execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"after rewind"}),
    )
    .await;
    assert_eq!(sent["accepted"], true);
    assert_eq!(
        request(
            execution,
            "agent.finish.wait.request",
            json!({"agentId":id})
        )
        .await["lastMessage"],
        "Claude: after rewind"
    );
}
