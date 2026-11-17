//! Physical connection progress while an admitted native operation is waiting.

use super::*;

#[derive(Debug)]
struct ResumeGate(std::path::PathBuf);

impl Drop for ResumeGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-resume"), "");
    }
}

async fn subscribed_socket(scoped: &Scoped, agent_id: &str) -> (Socket, Value) {
    let mut socket = scoped.fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"single-provider-progress",
            "protocol":{"major":1,"min_minor":0,"max_minor":0},
            "capabilities":scoped.fixture.api.shared.info().capabilities,
            "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    let subscribed = request(
        &mut socket,
        "agent.timeline.set_subscription.request",
        json!({"agentIds":[agent_id]}),
    )
    .await;
    let subscription = subscribed["result"]["subscriptionId"].clone();
    assert!(subscription.is_string(), "{subscribed}");
    (socket, subscription)
}

#[tokio::test]
async fn single_socket_releases_subscriptions_and_forks_while_another_agent_resumes() {
    let scoped = Scoped::start().await;
    std::fs::write(scoped.other_cwd.join("behavior"), "delayed-resume").unwrap();
    let gate = ResumeGate(scoped.other_cwd.clone());
    let resumable = scoped
        .rpc(
            "agent.create.request",
            json!({"workspaceId":scoped.other,"config":{"provider":"codex",
                "model":"offline-model","cwd":scoped.other_cwd}}),
        )
        .await;
    let handle = resumable["agent"]["persistence"].clone();
    assert!(handle.is_object(), "{resumable}");
    scoped
        .rpc(
            "agent.delete.request",
            json!({"agentId":resumable["agentId"]}),
        )
        .await;
    let sibling = scoped
        .agent_in(&scoped.workspace, &scoped.cwd, json!({}), Some("stored"))
        .await;
    scoped
        .rpc("agent.finish.wait.request", json!({"agentId":sibling}))
        .await;

    let (mut socket, subscription) = subscribed_socket(&scoped, &sibling).await;

    send(
        &mut socket,
        json!({"type":"request","request_id":"slow-resume",
            "method":"agent.resume.request","params":{"handle":handle,
                "overrides":{"cwd":scoped.other_cwd}}}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !scoped.other_cwd.join("resume-started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native resume must reach its explicit gate");

    for (id, method, params) in [
        (
            "fork",
            "agent.fork_context.request",
            json!({"agentId":sibling}),
        ),
        (
            "snapshot",
            "provider.snapshot.get.request",
            json!({"cwd":scoped.cwd}),
        ),
        (
            "release",
            "subscription.release.request",
            json!({"subscriptionId":subscription}),
        ),
    ] {
        send(
            &mut socket,
            json!({"type":"request","request_id":id,"method":method,"params":params}),
        )
        .await;
    }
    let mut remaining = BTreeSet::from(["fork", "snapshot", "release"]);
    while !remaining.is_empty() {
        let response = receive(&mut socket).await;
        if response["type"] == "event" {
            continue;
        }
        assert_eq!(response["type"], "response", "{response}");
        let id = response["request_id"].as_str().unwrap();
        assert!(remaining.remove(id), "unexpected response: {response}");
        match id {
            "fork" => assert!(
                response["result"]["attachment"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Echo: stored")
            ),
            "snapshot" => assert!(response["result"]["entries"].is_array()),
            "release" => assert_eq!(response["result"]["subscriptionId"], subscription),
            _ => unreachable!("response identity was checked above"),
        }
    }
    drop(gate);
    loop {
        let response = receive(&mut socket).await;
        if response["type"] != "event" {
            assert_eq!(response["request_id"], "slow-resume");
            assert_eq!(response["type"], "response", "{response}");
            break;
        }
    }
    assert_eq!(
        request(&mut socket, "connection.ping", json!({"nonce":"alive"})).await["result"]["nonce"],
        "alive"
    );
    socket.close(None).await.unwrap();
    scoped.stop().await;
}
