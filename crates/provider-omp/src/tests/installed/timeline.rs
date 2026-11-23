//! Timeline mapping (B6) against the official peer, driven by a loopback mock model.
use super::*;

/// The prompt that starts the scripted tool rounds.
const TRIGGER: &str = "OMP_TIMELINE_FIXTURE";

/// Launch the installed peer with a mock model that spends a known cost per request and a
/// cost warning threshold below it, so OMP reports a `notice`.
async fn installed_with_timeline_model() -> Installed {
    let fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(mock_timeline_completion),
    );
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let agent = fixture.root.path().join("agent");
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":1000,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(agent.join("config.yml"), "session:\n  warnCost: 0.001\n").unwrap();
    Installed {
        _server: server,
        ..fixture
    }
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_tools_todos_and_background_results_replay_as_shown_live() {
    let fixture = installed_with_timeline_model().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn(TRIGGER, &fixture.spec.config)
        .await
        .unwrap();
    // OMP settles only after the background job's result run.
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let live: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect();
    let tool = |name: &str| {
        live.iter()
            .find(|entry| entry.item["type"] == "tool_call" && entry.item["name"] == name)
            .unwrap_or_else(|| panic!("no {name} row: {live:?}"))
            .item
            .clone()
    };
    assert_eq!(tool("write")["detail"]["type"], "write");
    assert_eq!(
        tool("read")["detail"],
        json!({"type":"read","filePath":"probe.txt","content":"line one\nline two"})
    );
    let edit = tool("edit");
    assert_eq!(edit["status"], "completed", "{edit}");
    assert!(
        edit["detail"]["filePath"]
            .as_str()
            .unwrap()
            .ends_with("probe.txt")
    );
    assert!(
        edit["detail"]["unifiedDiff"]
            .as_str()
            .unwrap()
            .contains("+1|line 1")
    );
    assert_eq!(tool("bash")["detail"]["type"], "shell");
    assert!(live.iter().all(|entry| entry.item["name"] != "todo"));
    let todo = live
        .iter()
        .find(|entry| entry.key.contains(":todo:"))
        .unwrap_or_else(|| panic!("no todo list: {live:?}"));
    assert_eq!(todo.item["items"][0]["text"], "one");
    // Tools complete live from tool_execution_end, before their result messages.
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Progress { entry, .. } if entry.item["name"] == "edit"
            && entry.item["status"] == "completed")));
    let notices: Vec<_> = live
        .iter()
        .filter(|entry| entry.item["type"] == "notification")
        .map(|entry| entry.item["message"].as_str().unwrap())
        .collect();
    assert!(
        notices
            .iter()
            .any(|message| message.starts_with("Cost warning")),
        "{notices:?}"
    );
    assert!(
        notices
            .iter()
            .any(|message| message.starts_with("Background job bg_1")
                && message.contains("async-done")),
        "{notices:?}"
    );

    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    let recorded: Vec<_> = live
        .into_iter()
        .filter(|entry| !entry.key.contains(":notice:"))
        .collect();
    assert_eq!(history, recorded);
}

/// One scripted tool round per request after the trigger: write, read, a hashline edit
/// tagged by that read, then todos with a background job; later requests get text.
async fn mock_timeline_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let messages = request["messages"].as_array().unwrap();
    let last_user = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap_or_default();
    let current = &messages[last_user..];
    let round = current
        .iter()
        .filter(|message| message["role"] == "assistant" && message["tool_calls"].is_array())
        .count();
    let triggered = messages[last_user]["content"].to_string().contains(TRIGGER);
    let tag = current
        .iter()
        .rev()
        .filter(|message| message["role"] == "tool")
        .find_map(|message| {
            let text = message["content"].to_string();
            let start = text.find("probe.txt#")? + "probe.txt#".len();
            text.get(start..start + 4).map(str::to_owned)
        })
        .unwrap_or_default();
    let calls = match round {
        0 => vec![(
            "write",
            json!({"i":"w","path":"probe.txt","content":"line one\nline two\n"}),
        )],
        1 => vec![("read", json!({"i":"r","path":"probe.txt"}))],
        2 => vec![(
            "edit",
            json!({"i":"e","input":format!("[probe.txt#{tag}]\nPUT 1.=1:\n+line 1\n")}),
        )],
        3 => vec![
            (
                "todo",
                json!({"i":"t","op":"init","list":[{"phase":"Build","items":["one","two"]}]}),
            ),
            (
                "bash",
                json!({"i":"bg","command":"sleep 0.3; echo async-done","async":true}),
            ),
        ],
        _ => Vec::new(),
    };
    let delta = if triggered && !calls.is_empty() {
        let calls: Vec<_> = calls
            .iter()
            .enumerate()
            .map(|(index, (name, arguments))| {
                json!({"index":index,"id":format!("timeline-{round}-{index}"),"type":"function",
                    "function":{"name":name,"arguments":arguments.to_string()}})
            })
            .collect();
        json!({"role":"assistant","tool_calls":calls})
    } else {
        json!({"role":"assistant","content":"Fixture complete"})
    };
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
