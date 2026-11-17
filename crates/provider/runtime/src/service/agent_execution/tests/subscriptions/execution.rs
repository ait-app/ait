//! A slow native request must not occupy the physical connection's Provider worker.

use super::*;

struct ResumeGate(std::path::PathBuf);

impl Drop for ResumeGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-resume"), "");
    }
}

impl Peer {
    async fn dispatch(&mut self, id: &str, method: &str, params: Value) {
        let mut context = Some(Context {
            request: Request {
                id: id.to_owned(),
                method: method.to_owned(),
                params,
            },
            runtime: &self.state.runtime,
            outbound: &self.outbound,
            available_subscriptions: 8,
        });
        tokio::time::timeout(
            Duration::from_secs(3),
            crate::dispatch::dispatch(&mut context, &self.state, &mut self.connection),
        )
        .await
        .expect("Provider admission must not await native I/O")
        .unwrap();
        assert!(context.is_none());
    }

    async fn response(&mut self, id: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let queued = self.receiver.recv().await.unwrap();
                let Frame::Text(text) = &queued.message else {
                    panic!("expected a JSON response");
                };
                let value: Value = serde_json::from_str(text).unwrap();
                if value["type"] != "event" {
                    assert_eq!(value["request_id"], id, "{value}");
                    return value;
                }
            }
        })
        .await
        .expect("independent Provider requests must remain responsive")
    }
}

#[tokio::test]
async fn blocked_native_resume_allows_same_connection_cached_history_fork_and_subscriptions() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let first = create(&original, &fixture).await;
    let second = create(&original, &fixture).await;
    let first_id = first["agentId"].as_str().unwrap();
    for id in [first_id, second["agentId"].as_str().unwrap()] {
        original
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"stored"}),
            )
            .await
            .unwrap();
        original
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
    }
    original.shutdown().await.unwrap();
    fixture.mode("delayed-resume");
    let gate = ResumeGate(fixture.cwd.clone());
    let (execution, _) = worker(&fixture);
    let mut peer = Peer::new(&execution);

    peer.dispatch(
        "resume",
        "agent.resume.request",
        json!({"handle":first["agent"]["persistence"]}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/resume")
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the native resume reached its explicit gate");
    assert!(peer.receiver.try_recv().is_err());

    peer.dispatch(
        "cached",
        "agent.timeline.get.request",
        json!({"agentId":first_id,"allowCached":true,"limit":1}),
    )
    .await;
    let cached = peer.response("cached").await;
    assert_eq!(
        cached["result"]["entries"][0]["item"]["text"],
        "Echo: stored"
    );

    peer.dispatch(
        "fork",
        "agent.fork_context.request",
        json!({"agentId":second["agentId"]}),
    )
    .await;
    assert!(
        peer.response("fork").await["result"]["attachment"]["text"]
            .as_str()
            .unwrap()
            .contains("Echo: stored")
    );

    peer.dispatch(
        "subscribe",
        "agent.timeline.set_subscription.request",
        json!({"agentIds":[first_id]}),
    )
    .await;
    assert!(peer.response("subscribe").await["result"]["subscriptionId"].is_string());

    peer.dispatch(
        "snapshot",
        "provider.snapshot.get.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    assert!(peer.response("snapshot").await["result"]["entries"].is_array());
    drop(gate);
    assert_eq!(peer.response("resume").await["result"]["agentId"], first_id);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn execution_response_slots_are_bounded_until_delivery_and_preserve_command_order() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let mut first = execution
        .admit_request(
            "agent.model.set.request",
            json!({"agentId":id,"modelId":"first-model"}),
        )
        .unwrap();
    let mut second = execution
        .admit_request(
            "agent.model.set.request",
            json!({"agentId":id,"modelId":"second-model"}),
        )
        .unwrap();
    assert_eq!(second.receive().await.unwrap()["accepted"], true);
    assert_eq!(first.receive().await.unwrap()["accepted"], true);
    let agent = execution
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(agent["agent"]["model"], "second-model");
    let mut responses = vec![first, second];
    for _ in 2..64 {
        let mut response = execution
            .admit_request("agent.get.request", json!({"agentId":id}))
            .unwrap();
        response.receive().await.unwrap();
        responses.push(response);
    }
    assert!(matches!(
        execution.admit_request("agent.get.request", json!({"agentId":id})),
        Err(ErrorCode::CatalogBusy)
    ));
    drop(responses.pop());
    let mut replacement = execution
        .admit_request("agent.get.request", json!({"agentId":id}))
        .unwrap();
    replacement.receive().await.unwrap();
    assert!(matches!(
        execution.admit_request("agent.finish.wait.request", json!({"agentId":id})),
        Err(ErrorCode::UnsupportedCapability)
    ));
    assert!(matches!(
        execution.admit_request("provider.snapshot.get.request", json!({})),
        Err(ErrorCode::UnsupportedCapability)
    ));
    drop(replacement);
    drop(responses);
    execution.shutdown().await.unwrap();
    assert!(matches!(
        execution.admit_request("agent.get.request", json!({"agentId":id})),
        Err(ErrorCode::AgentIo)
    ));
}
