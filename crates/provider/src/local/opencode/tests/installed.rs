use super::super::client::OpenCodeClient;
use super::super::{protocol::Version, runtime};
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionSpec, AgentTurnEvent,
};
use axum::{Json, Router, routing::post};
use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio_util::task::AbortOnDropHandle;

mod concurrent;

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated config and loopback model only"]
async fn installed_opencode_discovers_runs_and_restores_with_local_model() {
    let (_root, client, spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(answer))).await;
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(
        details
            .models
            .iter()
            .any(|model| model["id"] == "local/test-model")
    );
    assert!(
        details
            .models
            .iter()
            .any(|model| model["id"] == "second/test-model")
    );
    let mut session = client.create_session(&spec).await.unwrap();
    for prompt in ["first turn", "second turn"] {
        session.start_turn(prompt, &spec.config).await.unwrap();
        assert_completed(session.as_mut()).await;
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 100,
        })
        .await
        .unwrap();
    assert!(
        listed
            .iter()
            .any(|entry| entry.provider_handle_id == handle.session_id)
    );
    let mut imported = domain::agent_runtime::AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: handle.session_id.clone(),
        native_handle: None,
        metadata: None,
    };
    let inspected = client.inspect_session(&imported, &spec.cwd).await.unwrap();
    assert_eq!(inspected.entries.len(), 4);
    imported.metadata = Some(inspected.resume_metadata);
    let imported_spec = AgentSessionSpec {
        config: inspected.config,
        ..spec.clone()
    };
    let mut imported_session = client
        .resume_session(&imported, &imported_spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        imported_session.persistence().unwrap().session_id,
        handle.session_id
    );
    imported_session
        .start_turn("after import", &imported_spec.config)
        .await
        .unwrap();
    assert_completed(imported_session.as_mut()).await;
    imported_session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert_eq!(history.len(), 6);
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn("after restart", &spec.config)
        .await
        .unwrap();
    assert_completed(resumed.as_mut()).await;
    resumed.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 8);
}

fn isolated_binary(root: &Path, binary: &str) -> std::path::PathBuf {
    let environment = json!({
        "XDG_DATA_HOME":root.join("data"), "XDG_CONFIG_HOME":root.join("config"),
        "XDG_CACHE_HOME":root.join("cache"), "XDG_STATE_HOME":root.join("state"),
        "OPENCODE_DISABLE_AUTOUPDATE":"1", "OPENCODE_DISABLE_MODELS_FETCH":"1"
    });
    let binary = serde_json::to_string(binary).unwrap();
    let script = format!(
        "#!/usr/bin/env python3\nimport os, sys\nenv = dict(os.environ)\nenv.update({environment})\nos.execve({binary}, [{binary}, *sys.argv[1:]], env)\n"
    );
    let path = root.join("isolated-opencode");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

async fn answer() -> ([(&'static str, &'static str); 1], String) {
    let chunks = [
        json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"Local deterministic answer."},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
    ];
    let body = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunks[0], chunks[1]
    );
    ([("content-type", "text/event-stream")], body)
}

async fn assert_completed(session: &mut dyn AgentSession) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::Completed(text)) => {
                    assert_eq!(text.as_deref(), Some("Local deterministic answer."));
                    return;
                }
                Some(AgentTurnEvent::Failed | AgentTurnEvent::Cancelled) => panic!("turn failed"),
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap();
}

async fn installed_fixture(
    router: Router,
) -> (
    tempfile::TempDir,
    OpenCodeClient,
    AgentSessionSpec,
    AbortOnDropHandle<()>,
) {
    let binary = std::env::var("AIT_TEST_OPENCODE_BIN").expect("set AIT_TEST_OPENCODE_BIN");
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("workspace + & 测试");
    std::fs::create_dir(&cwd).unwrap();
    let cwd = cwd.canonicalize().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let wrapper = isolated_binary(root.path(), &binary);
    let version = runtime::probe(&wrapper, &cwd, &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    let mut config = match version {
        Version::V1 => json!({"provider":{"local":{
            "name":"Local test", "npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":format!("http://{address}/v1"),"apiKey":"local-only"},
            "models":{"test-model":{"name":"Test","limit":{"context":32000,"output":2000}}}
        }}, "permission":{"*":"ask"}}),
        Version::V2 => json!({"providers":{"local":{
            "name":"Local test", "package":"@opencode/ai/providers/openai-compatible",
            "settings":{"baseURL":format!("http://{address}/v1"),"apiKey":"local-only"},
            "models":{"test-model":{"name":"Test","limit":{"context":32000,"output":2000}}}
        }}, "permissions":[{"action":"shell", "resource":"*", "effect":"ask"}]}),
    };
    let providers = match version {
        Version::V1 => "provider",
        Version::V2 => "providers",
    };
    config[providers]["second"] = config[providers]["local"].clone();
    std::fs::write(cwd.join("opencode.json"), config.to_string()).unwrap();
    // Exercise the actual cold-start catalog, rather than warming it with a separate probe.
    let client = OpenCodeClient::new(wrapper);
    let spec = AgentSessionSpec {
        provider: "opencode".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        config: StoredAgentConfig {
            model: Some("local/test-model".into()),
            ..Default::default()
        },
    };
    (root, client, spec, server)
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated config and loopback model only"]
async fn installed_opencode_declined_tool_settles_and_accepts_next_turn() {
    let (_root, client, spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(tool_answer))).await;
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("request a tool", &spec.config)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut denied = false;
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::PermissionRequested(request)) => {
                    session
                        .respond_permission(
                            request["id"].as_str().unwrap(),
                            &json!({"behavior":"deny","selectedActionId":"deny"}),
                        )
                        .await
                        .unwrap();
                    denied = true;
                }
                Some(AgentTurnEvent::Cancelled) => {
                    assert!(denied);
                    break;
                }
                Some(event @ (AgentTurnEvent::Failed | AgentTurnEvent::Completed(_))) => {
                    panic!("unexpected terminal event: {event:?}")
                }
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .expect("denial must settle without manually cancelling");
    session
        .start_turn("after denial", &spec.config)
        .await
        .unwrap();
    assert_completed(session.as_mut()).await;
    session.close().await.unwrap();
}

async fn tool_answer(Json(body): Json<Value>) -> ([(&'static str, &'static str); 1], String) {
    let after_denial = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .is_some_and(|message| message["content"].to_string().contains("after denial"));
    if after_denial {
        return answer().await;
    }
    if body["tools"].as_array().is_none_or(Vec::is_empty) {
        // Native metadata requests do not expose tools; permission tests below
        // still require a real foreground request and native approval round trip.
        return answer().await;
    }
    let tool = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
        .find(|name| matches!(*name, "bash" | "shell"))
        .expect("OpenCode must advertise its native command tool");
    let delta = json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{
        "index":0,"id":"call-denied","type":"function","function":{"name":tool,"arguments":"{\"command\":\"pwd\"}"}
    }]},"finish_reason":null}]});
    let finish = json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]});
    (
        [("content-type", "text/event-stream")],
        format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n"),
    )
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated config and loopback model only"]
async fn installed_opencode_plan_import_and_switch_use_native_agent() {
    let (_root, client, mut spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(answer))).await;
    spec.config.mode_id = Some("plan".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("plan only", &spec.config).await.unwrap();
    assert_completed(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    let external = domain::agent_runtime::AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: handle.session_id.clone(),
        native_handle: None,
        metadata: None,
    };
    let history = client.inspect_session(&external, &spec.cwd).await.unwrap();
    assert_eq!(history.config.mode_id.as_deref(), Some("plan"));
    spec.config.mode_id = Some("build".into());
    session.start_turn("build now", &spec.config).await.unwrap();
    assert_completed(session.as_mut()).await;
    let history = client.inspect_session(&external, &spec.cwd).await.unwrap();
    assert_eq!(history.config.mode_id.as_deref(), Some("build"));
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated config and loopback model only"]
async fn installed_opencode_saves_only_explicit_native_permission_rules() {
    let (_root, client, spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(saved_tool_answer)))
            .await;
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("request a tool", &spec.config)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut allowed = false;
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::PermissionRequested(request)) => {
                    assert!(!allowed, "one tool request must not ask twice");
                    assert!(
                        request["actions"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|action| action["id"] == "always"),
                        "{request}"
                    );
                    assert!(
                        !request["input"]["saveResources"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                    session
                        .respond_permission(
                            request["id"].as_str().unwrap(),
                            &json!({"behavior":"allow","selectedActionId":"always"}),
                        )
                        .await
                        .unwrap();
                    allowed = true;
                }
                Some(AgentTurnEvent::Completed(_)) => {
                    assert!(allowed);
                    break;
                }
                Some(AgentTurnEvent::Failed | AgentTurnEvent::Cancelled) => panic!("turn failed"),
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
}

async fn saved_tool_answer(Json(body): Json<Value>) -> ([(&'static str, &'static str); 1], String) {
    if body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool")
    {
        answer().await
    } else {
        tool_answer(Json(body)).await
    }
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; private loopback model, no credentials"]
async fn installed_opencode_metadata_disables_tools_and_removes_private_history() {
    async fn metadata_answer(
        Json(request): Json<Value>,
    ) -> ([(&'static str, &'static str); 1], String) {
        assert!(
            request["tools"].as_array().is_none_or(Vec::is_empty),
            "metadata must not advertise tools"
        );
        answer().await
    }
    let (_root, client, spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(metadata_answer))).await;
    let result = tokio::time::timeout(
        Duration::from_secs(40),
        client.generate_summary(&spec, "Generate a title", &json!({"type":"object"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, "Local deterministic answer.");
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd),
            scan_limit: 100,
        })
        .await
        .unwrap();
    assert!(listed.is_empty());
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated native permissions and loopback model"]
async fn installed_opencode_permission_control_uses_native_allow_ask_and_deny() {
    let (_root, client, mut spec, _server) = installed_fixture(
        Router::new().route("/v1/chat/completions", post(permission_control_answer)),
    )
    .await;
    for effect in ["allow", "ask", "deny"] {
        spec.config.feature_values = Some(std::collections::BTreeMap::from([(
            "permission".into(),
            json!(effect),
        )]));
        let mut session = client.create_session(&spec).await.unwrap();
        session
            .start_turn(&format!("permission-case-{effect}"), &spec.config)
            .await
            .unwrap();
        let asks = tokio::time::timeout(Duration::from_secs(30), async {
            let mut asks = 0;
            loop {
                match session.poll_turn().unwrap() {
                    Some(AgentTurnEvent::PermissionRequested(request)) => {
                        assert_eq!(effect, "ask");
                        asks += 1;
                        session
                            .respond_permission(
                                request["id"].as_str().unwrap(),
                                &json!({"behavior":"allow","selectedActionId":"allow"}),
                            )
                            .await
                            .unwrap();
                    }
                    Some(AgentTurnEvent::Completed(_)) => break asks,
                    Some(event @ (AgentTurnEvent::Failed | AgentTurnEvent::Cancelled)) => {
                        panic!("{effect}: {event:?}")
                    }
                    _ => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(asks > 0, effect == "ask");
        session.close().await.unwrap();
    }
}

async fn permission_control_answer(
    Json(body): Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let has_shell = body["tools"].as_array().is_some_and(|tools| {
        tools.iter().any(|tool| {
            matches!(
                tool.pointer("/function/name").and_then(Value::as_str),
                Some("bash" | "shell")
            )
        })
    });
    let deny_case = body["messages"].as_array().unwrap().iter().any(|message| {
        message["role"] == "user"
            && message["content"]
                .to_string()
                .contains("permission-case-deny")
    });
    if deny_case {
        assert!(
            !has_shell,
            "native deny must remove shell from available tools"
        );
    }
    if has_shell {
        saved_tool_answer(Json(body)).await
    } else {
        answer().await
    }
}
