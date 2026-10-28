use super::*;
use crate::service::agent_execution::AgentExecution;
use crate::test_support::{Fixture, request, worker_with_client};

fn worker(fixture: &Fixture, program: PathBuf) -> AgentExecution {
    worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(
            MiniMaxClient::new(program).with_image_directory(fixture.root.path().join("images")),
        ),
    )
    .0
}

async fn discovered_model(execution: &AgentExecution, fixture: &Fixture, live: bool) -> Value {
    let models = request(
        execution,
        "provider.models.list.request",
        json!({"provider":PROVIDER,"cwd":fixture.cwd}),
    )
    .await;
    if !live {
        let snapshot = request(
            execution,
            "provider.snapshot.get.request",
            json!({"cwd":fixture.cwd}),
        )
        .await;
        let rejected = snapshot["entries"][0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["id"] == "minimax/rejected")
            .expect("rejected model remains visible in diagnostics");
        assert_eq!(rejected["isSelectable"], false);
        assert_eq!(models["models"].as_array().unwrap().len(), 2);
        assert!(
            models["models"]
                .as_array()
                .unwrap()
                .iter()
                .all(|model| model["id"] != "minimax/rejected")
        );
    }
    models["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["isSelectable"] != false)
        .expect("discovery advertises a model that can create a session")
        .clone()
}

/// Exercise the same catalog, registry, scheduling and timeline API used by AIT clients.
/// Uses the isolated `fixture` and `program`; `live` selects model prompts over fixture commands.
pub(super) async fn round_trip(fixture: &Fixture, program: PathBuf, live: bool) {
    let execution = worker(fixture, program.clone());
    let model = discovered_model(&execution, fixture, live).await;
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":PROVIDER,"cwd":fixture.cwd,
            "model":model["id"],"thinkingOptionId":model["defaultThinkingOptionId"]}}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    let prompt = if live {
        "Remember the code AIT_MCODE_7391 for our next message. \
         Reply exactly AIT_MCODE_7391. Do not use tools."
    } else {
        "remember"
    };
    request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":prompt}),
    )
    .await;
    let finished = tokio::time::timeout(
        Duration::from_secs(180),
        execution.execute("agent.finish.wait.request", json!({"agentId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    let expected = if live { "AIT_MCODE_7391" } else { "remembered" };
    assert!(
        finished["lastMessage"]
            .as_str()
            .is_some_and(|message| message.contains(expected)),
        "{finished}"
    );
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(timeline.to_string().contains(expected), "{timeline}");
    execution.shutdown().await.unwrap();

    let execution = worker(fixture, program);
    let resumed = request(
        &execution,
        "agent.resume.request",
        json!({"handle":created["agent"]["persistence"]}),
    )
    .await;
    assert_eq!(resumed["agentId"], id, "{resumed}");
    let prompt = if live {
        "What code did I ask you to remember? Reply only with that code. Do not use tools."
    } else {
        "recall"
    };
    request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":prompt}),
    )
    .await;
    let finished = tokio::time::timeout(
        Duration::from_secs(180),
        execution.execute("agent.finish.wait.request", json!({"agentId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        finished["lastMessage"]
            .as_str()
            .is_some_and(|message| message.contains(expected)),
        "{finished}"
    );
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(timeline.to_string().contains(expected), "{timeline}");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn ait_catalog_execution_timeline_and_restart_preserve_conversation() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cwd.join("scenario"), "rejected-model").unwrap();
    let program =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/local/minimax/tests/fixtures/acp.py");
    round_trip(&fixture, program, false).await;
}
