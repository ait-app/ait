use super::*;
use std::fmt::Write as _;

#[test]
fn rejects_unknown_compositions_without_starting_a_foreground_session() {
    assert_eq!(
        isolated_patch(
            "- id: malicious/path",
            &["p".into(), "m".into()],
            "source",
            &json!({})
        ),
        Err(AgentSessionError::Failed)
    );
    assert_eq!(
        isolated_patch(
            "- id: tools",
            &["p".into(), "m".into()],
            "source",
            &json!({})
        ),
        Err(AgentSessionError::Unavailable)
    );
}

#[test]
fn disables_every_unknown_plugin_and_uses_in_memory_sessions() {
    let ids = [
        "timer",
        "llm",
        "deepseek-llm-api-extensions",
        "session",
        "session-log-deepseek",
        "session-title",
        "agent",
        "agent-default-model",
        "llm-retry",
        "credentials",
        "llm-pi-ai",
        "llm-deepseek",
        "session-projection",
        "tools",
        "system-prompt",
        "agent-loop",
        "headless-runner",
        "session-persistence-jsonl",
        "user-custom-tools",
        "session-title-llm",
        "headless-startup",
    ];
    let config = ids.iter().fold(String::new(), |mut config, id| {
        config.push_str("- id: ");
        config.push_str(id);
        config.push('\n');
        config
    });
    let patch = isolated_patch(
        &config,
        &["p".into(), "m".into()],
        "source",
        &json!({"type":"object"}),
    )
    .unwrap();
    let entries = patch.as_array().unwrap();
    for disabled in [
        "session-persistence-jsonl",
        "user-custom-tools",
        "session-title-llm",
        "headless-startup",
    ] {
        assert_eq!(
            entries
                .iter()
                .rfind(|entry| entry["id"] == disabled)
                .unwrap()["disabled"],
            true
        );
    }
    assert_eq!(
        entries
            .iter()
            .rfind(|entry| entry["id"] == "headless-runner")
            .unwrap()["inject"],
        json!([])
    );
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN; loopback model and temporary native profile only"]
async fn installed_dsh_metadata_has_no_tools_or_persisted_session() {
    use crate::ports::agent_session::AgentClient;
    use crate::ports::environment::AgentEnvironment;
    use axum::{Json, Router, routing::post};
    use std::{collections::BTreeMap, time::Duration};
    async fn answer(Json(body): Json<Value>) -> ([(&'static str, &'static str); 1], String) {
        assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
        (
            [("content-type", "text/event-stream")],
            format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"id":"local","model":"deepseek-flash","choices":[{"index":0,"delta":{"role":"assistant","content":"{\"title\":\"DSH metadata\"}"},"finish_reason":null}]}),
                json!({"id":"local","model":"deepseek-flash","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
            ),
        )
    }
    async fn messages(Json(body): Json<Value>) -> ([(&'static str, &'static str); 1], String) {
        assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
        let events = [
            json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"title\":\"DSH metadata\"}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ];
        let mut stream = String::new();
        for event in events {
            writeln!(
                stream,
                "event: {}\ndata: {event}\n",
                event["type"].as_str().unwrap()
            )
            .unwrap();
        }
        ([("content-type", "text/event-stream")], stream)
    }
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/chat/completions", post(answer))
                .route("/v1/messages", post(messages)),
        )
        .await
        .unwrap();
    }));
    let mut client =
        DeepSeekHarnessClient::new(std::env::var_os("AIT_TEST_DSH_BIN").unwrap().into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        ("DSH_HOME".into(), root.path().to_str().unwrap().into()),
        ("DEEPSEEK_API_KEY".into(), "loopback-only".into()),
    ]))
    .unwrap();
    let cwd = root.path().to_str().unwrap();
    let mut initialize = client.metadata_command(cwd);
    initialize.args(["--profile", "headless", "--dump-config"]);
    metadata_process::output(&mut initialize).await.unwrap();
    std::fs::write(root.path().join("profiles/headless/cordis.patch.yml"), json!([
        {"id":"llm-deepseek","config":{"baseURL":format!("http://{address}"),"thinking":"disabled"}}
    ]).to_string()).unwrap();
    let spec = AgentSessionSpec {
        provider: super::super::PROVIDER.into(),
        cwd: cwd.into(),
        config: domain::agent_runtime::StoredAgentConfig {
            model: Some(json!(["deepseek-official", "deepseek-flash"]).to_string()),
            ..Default::default()
        },
    };
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        client.generate_metadata(&spec, "Generate a title", &json!({"type":"object"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.trim(), "{\"title\":\"DSH metadata\"}");
    assert!(!root.path().join("sessions").exists());
    drop(server);
}
