use super::*;
use base64::Engine;

#[tokio::test]
async fn initial_user_image_preview_survives_native_delivery_refresh_and_restart() {
    let fixture = Fixture::new();
    fixture.mode("rich-user-images");
    let (execution, _) = worker(&fixture);
    let created = execution
        .execute(
            "agent.create.request",
            json!({
                "config":{"provider":"codex","cwd":fixture.cwd},
                "initialPrompt":"这是什么植物？", "clientMessageId":"mobile-input",
                "images":[{"data":"aGVsbG8=","mimeType":"image/png"}]
            }),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let before = user_image_preview(&execution, id).await;
    assert_eq!(
        std::fs::read(before["imageSources"][0].as_str().unwrap()).unwrap(),
        b"hello"
    );
    let requests = fixture.requests();
    let input = &requests
        .iter()
        .find(|request| request["method"] == "turn/start")
        .unwrap()["params"]["input"];
    assert_eq!(input.as_array().unwrap().len(), 2);
    assert_eq!(input[0]["text"], "这是什么植物？");
    assert_eq!(input[1]["url"], "data:image/png;base64,aGVsbG8=");
    execution
        .execute("agent.refresh.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(user_image_preview(&execution, id).await, before);
    execution.shutdown().await.unwrap();

    let (restarted, _) = worker(&fixture);
    assert_eq!(user_image_preview(&restarted, id).await, before);
    restarted.shutdown().await.unwrap();
}

async fn user_image_preview(execution: &AgentExecution, id: &str) -> Value {
    let page = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    let item = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["item"]["type"] == "user_message")
        .unwrap()["item"]
        .clone();
    assert_eq!(item["text"], "这是什么植物？");
    assert_eq!(item["imageSources"].as_array().unwrap().len(), 1);
    item
}

#[tokio::test]
async fn screenshot_message_reaches_native_provider_once_for_interrupt_and_steer() {
    for behavior in ["interrupt", "steer"] {
        let fixture = Fixture::new();
        let (execution, _) = worker(&fixture);
        let created = create(&execution, &fixture).await;
        let id = created["agentId"].as_str().unwrap();
        execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"hang"}),
            )
            .await
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(vec![0; 3_886_681]);
        let request = json!({"agentId":id,"text":"Inspect this screenshot",
            "activeTurnBehavior":behavior,"messageId":"screenshot",
            "images":[{"data":data,"mimeType":"image/png"}]});
        for _ in 0..2 {
            let response = execution
                .execute("agent.message.send.request", request.clone())
                .await
                .unwrap();
            assert_eq!(response["accepted"], true);
        }
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
        let requests = fixture.requests();
        let delivered: Vec<_> = requests
            .iter()
            .filter(|request| request["params"]["clientUserMessageId"] == "screenshot")
            .collect();
        assert_eq!(delivered.len(), 1);
        assert_eq!(
            delivered[0]["method"],
            if behavior == "interrupt" {
                "turn/start"
            } else {
                "turn/steer"
            }
        );
        assert_eq!(
            delivered[0]["params"]["input"][1]["url"],
            format!("data:image/png;base64,{data}")
        );
        execution.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn creation_and_message_requests_deliver_rich_content_and_reject_bad_payloads_before_launch()
{
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let images = json!([{"data":"aGVsbG8=","mimeType":"image/png"}]);
    let config = json!({"provider":"codex","cwd":fixture.cwd,"providerOptions":{"web_search":"live"},
        "mcpServers":{"docs":{"type":"stdio","command":"node"}},"toolPolicy":{"preapproved":[]}});
    assert!(
        execution
            .execute(
                "agent.create.request",
                json!({"config":config,"images":[{"data":"invalid","mimeType":"image/png"}]})
            )
            .await
            .is_err()
    );
    assert_eq!(
        registry.list().unwrap(),
        [] as [domain::agent_runtime::PersistedAgentRuntimeRecord; 0]
    );
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":config,"images":images,"initialPrompt":"Inspect",
        "attachments":[{"type":"text","mimeType":"text/plain","text":"context"}]}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let response = execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"", "images":images,
        "messageId":"client-input-1","outputSchema":{"type":"object"}}),
        )
        .await
        .unwrap();
    assert_eq!(response["accepted"], true);
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let requests = fixture.requests();
    let starts: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "turn/start")
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["params"]["input"][2]["text"], "context");
    assert_eq!(
        starts[1]["params"]["input"][0]["url"],
        "data:image/png;base64,aGVsbG8="
    );
    assert_eq!(starts[1]["params"]["clientUserMessageId"], "client-input-1");
    assert_eq!(
        starts[1]["params"]["outputSchema"],
        json!({"type":"object"})
    );
    execution.shutdown().await.unwrap();
}
