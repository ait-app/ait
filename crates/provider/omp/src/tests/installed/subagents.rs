//! Subagent transcripts, listing and history (B7) against the official peer, driven by a
//! loopback mock model.
use super::*;
use provider_core::ports::controls::SubagentEvent;
use provider_core::protocol::timeline::NativeItem;

/// The prompt that starts the parent's task round.
const TRIGGER: &str = "OMP_SUBAGENT_FIXTURE";
/// The task text that marks the child's requests.
const CHILD: &str = "OMP_CHILD_JOB";

async fn installed_with_task_model() -> Installed {
    let fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(mock_task_completion),
    );
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    std::fs::write(
        fixture.root.path().join("agent/models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    Installed {
        _server: server,
        ..fixture
    }
}

fn without_users(entries: &[NativeItem]) -> Vec<NativeItem> {
    // Live child user messages keep anchor ids (B4); history names them by native entries.
    entries
        .iter()
        .filter(|entry| entry.item["type"] != "user_message")
        .cloned()
        .collect()
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_subagent_transcripts_replay_and_list_as_shown_live() {
    let fixture = installed_with_task_model().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn(TRIGGER, &fixture.spec.config)
        .await
        .unwrap();
    // OMP 18.18.8 runs the task in the background and settles after its result run.
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let parent = session.persistence().unwrap();
    let live = session.subagents();
    session.close().await.unwrap();

    let id = format!("{}/Prober", parent.session_id);
    assert_eq!(live.len(), 1, "{live:?}");
    let child = &live[0];
    assert_eq!(child.id, id);
    assert_eq!(child.parent_id, parent.session_id);
    assert_eq!(
        child.descriptor["title"],
        "task · fixture (ait-local-fixture)"
    );
    assert_eq!(
        child.descriptor["description"],
        format!("# Target\n{CHILD} reply")
    );
    assert_eq!(child.descriptor["status"], "completed");
    let card = events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["name"] == "task" => Some(entry),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no task card: {events:?}"));
    assert_eq!(card.item["detail"]["childSessionId"], id.as_str());
    let live_child: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) => Some(entry.clone()),
            _ => None,
        })
        .collect();
    assert!(
        live_child
            .iter()
            .any(|entry| entry.item["detail"]["type"] == "shell"),
        "{live_child:?}"
    );

    assert_transcript_matches(&fixture, child, &live_child).await;

    // The parent replays as shown live, and only the parent is listed.
    let parent_history = fixture
        .client
        .history(&parent, &fixture.spec.cwd)
        .await
        .unwrap();
    let recorded: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if !entry.key.contains(":notice:") => {
                Some(entry.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(parent_history, recorded);
    let sessions = fixture
        .client
        .list_sessions(&ListOptions {
            cwd: Some(fixture.spec.cwd.clone()),
            scan_limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].provider_handle_id, parent.session_id);
    assert_eq!(sessions[0].first_prompt_preview.as_deref(), Some(TRIGGER));
    assert_eq!(sessions[0].title.as_deref(), Some(TRIGGER));
}

/// The child transcript is found from the parent's records, as OMP wrote it, and replays
/// the rows shown live.
async fn assert_transcript_matches(
    fixture: &Installed,
    child: &provider_core::ports::controls::NativeSubagent,
    live_child: &[NativeItem],
) {
    let discovered = fixture.client.subagents(&fixture.spec.cwd).await.unwrap();
    assert_eq!(discovered.len(), 1, "{discovered:?}");
    let found = &discovered[0];
    assert_eq!(found.id, child.id);
    assert_eq!(found.parent_id, child.parent_id);
    assert_eq!(found.persistence, child.persistence);
    assert_eq!(found.descriptor["title"], child.descriptor["title"]);
    assert_eq!(
        found.descriptor["description"],
        child.descriptor["description"]
    );
    assert_eq!(found.descriptor["status"], "completed");
    assert_eq!(
        found.descriptor["toolCallId"],
        child.descriptor["toolCallId"]
    );
    let history = fixture
        .client
        .inspect_session(found.persistence.as_ref().unwrap(), &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(history.parent_id.as_deref(), Some(child.parent_id.as_str()));
    assert_eq!(history.descriptor.provider_handle_id, child.id);
    assert_eq!(without_users(&history.entries), without_users(live_child));
}

/// The parent's first request starts the `Prober` task; the child's first request runs a
/// shell command; every later request gets text.
async fn mock_task_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let messages = request["messages"].as_array().unwrap();
    let last_user = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap_or_default();
    let round = messages[last_user..]
        .iter()
        .filter(|message| message["role"] == "assistant" && message["tool_calls"].is_array())
        .count();
    let text = messages[last_user]["content"].to_string();
    let call = match round {
        0 if text.contains(CHILD) => Some(("bash", json!({"i":"b","command":"echo child"}))),
        0 if text.contains(TRIGGER) => Some((
            "task",
            json!({"i":"delegate","context":"# Goal\nprobe",
                "tasks":[{"task":format!("# Target\n{CHILD} reply"),"name":"Prober"}]}),
        )),
        _ => None,
    };
    let delta = call.map_or_else(
        || json!({"role":"assistant","content":"Fixture complete"}),
        |(name, arguments)| {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("{name}-call"),
                "type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
        },
    );
    let finish = if delta["tool_calls"].is_array() {
        "tool_calls"
    } else {
        "stop"
    };
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":{},"finish_reason":finish}],
        "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}
