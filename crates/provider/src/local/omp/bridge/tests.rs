use super::*;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio_util::task::AbortOnDropHandle;

struct Peer {
    input: WriteHalf<DuplexStream>,
    output: BufReader<ReadHalf<DuplexStream>>,
    task: AbortOnDropHandle<std::io::Result<()>>,
    sequence: u64,
}

impl Peer {
    fn new(client: OmpClient) -> Self {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(server_io);
        let task = AbortOnDropHandle::new(tokio::spawn(serve(client, read, write)));
        let (read, write) = tokio::io::split(client_io);
        Self {
            input: write,
            output: BufReader::new(read),
            task,
            sequence: 0,
        }
    }
    async fn send(&mut self, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.unwrap();
    }
    async fn begin(&mut self, method: &str, params: Value) -> Value {
        self.sequence += 1;
        let id = json!(self.sequence);
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        id
    }
    async fn next(&mut self) -> Value {
        let mut line = String::new();
        let bytes = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(bytes > 0, "bridge closed");
        serde_json::from_str(&line).unwrap()
    }
    async fn until(&mut self, id: &Value) -> Vec<Value> {
        let mut messages = Vec::new();
        loop {
            let message = self.next().await;
            let done = message.get("method").is_none() && message["id"] == *id;
            messages.push(message);
            if done {
                return messages;
            }
        }
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.begin(method, params).await;
        self.until(&id).await.pop().unwrap()
    }
    async fn open(&mut self, cwd: &str, native: bool) -> String {
        let initialized = self
            .request(
                "initialize",
                json!({"protocolVersion":1,
            "clientCapabilities":{"elicitation":{"form":{}},"_meta":{"ompNative":native}},
            "clientInfo":{"name":"generic-acp-client","version":"1"}}),
            )
            .await;
        assert_eq!(initialized["result"]["_meta"]["backend"]["protocol"], "rpc");
        let created = self
            .request("session/new", json!({"cwd":cwd,"mcpServers":[]}))
            .await;
        assert!(created.get("error").is_none(), "{created}");
        created["result"]["sessionId"].as_str().unwrap().to_owned()
    }
    async fn finish(mut self, session: &str) {
        assert!(
            self.request("session/close", json!({"sessionId":session}))
                .await
                .get("result")
                .is_some()
        );
        self.input.shutdown().await.unwrap();
        self.task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn standard_acp_waits_for_native_background_settlement_and_deduplicates_chunks() {
    let (root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, false).await;
    let id = peer
        .begin(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"background"}]}),
        )
        .await;
    let messages = peer.until(&id).await;
    assert_eq!(messages.last().unwrap()["result"]["stopReason"], "end_turn");
    let text = messages
        .iter()
        .filter(|message| message["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
        .map(|message| {
            message["params"]["update"]["content"]["text"]
                .as_str()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(text, vec!["Inspect", "Hello world", "Background finished"]);
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let launches = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| {
            call["args"].is_array()
                && !call["args"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("--no-session"))
        })
        .collect::<Vec<_>>();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0]["args"][1], "rpc-ui");
    peer.finish(&session).await;
}

#[tokio::test]
async fn arbitrary_plugin_commands_can_ask_before_native_admission_and_keep_custom_output() {
    let (root, client, spec) = super::super::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        json!([
        {"name":"vendor-one-choose","description":"First extension","source":"extension"},
        {"name":"unrelated-other-command","description":"Other extension","source":"extension"}])
        .to_string(),
    )
    .unwrap();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, true).await;
    for command in ["vendor-one-choose", "unrelated-other-command"] {
        let id=peer.begin("session/prompt",json!({"sessionId":session,"prompt":[{"type":"text","text":format!("/{command}")}]})).await;
        let mut messages = Vec::new();
        loop {
            let message = peer.next().await;
            if message["method"] == "elicitation/create" {
                assert_eq!(
                    message["params"]["requestedSchema"]["properties"]["value"]["enum"],
                    json!(["A, B", "C"])
                );
                peer.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{"action":"accept","content":{"value":"A, B"}}})).await;
            }
            let done = message.get("method").is_none() && message["id"] == id;
            messages.push(message);
            if done {
                break;
            }
        }
        assert_eq!(messages.last().unwrap()["result"]["stopReason"], "end_turn");
        assert!(
            messages
                .iter()
                .any(|message| message["params"]["update"]["content"]["text"]
                    == format!("{command}: A, B"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message["method"] == "_omp/event"
                    && message["params"]["frame"]["opaque"]["vendor"] == command)
        );
    }
    peer.finish(&session).await;
}

#[tokio::test]
async fn malformed_acp_does_not_launch_native_or_replay_requests() {
    let (root, client, _spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    peer.input.write_all(b"{broken\n").await.unwrap();
    assert_eq!(peer.next().await["error"]["code"], -32700);
    let rejected = peer
        .request("session/new", json!({"cwd":root.path(),"mcpServers":[]}))
        .await;
    assert!(rejected.get("error").is_some());
    assert!(!root.path().join("agent/calls.jsonl").exists());
    peer.input.shutdown().await.unwrap();
    peer.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn standard_config_controls_and_rpc_extension_keep_native_ownership() {
    let (root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, true).await;
    for (id, value) in [
        ("model", json!(["other", "model"]).to_string()),
        ("thinking", "max".to_owned()),
    ] {
        let result = peer
            .request(
                "session/set_config_option",
                json!({"sessionId":session,"configId":id,"value":value}),
            )
            .await;
        assert!(result.get("result").is_some(), "{result:?}");
    }
    let state = peer
        .request(
            "_omp/rpc",
            json!({"sessionId":session,"command":"get_state"}),
        )
        .await;
    assert_eq!(state["result"]["sessionId"], session);
    assert_eq!(state["result"]["model"]["provider"], "other");
    let selected = peer
        .request(
            "session/set_config_option",
            json!({"sessionId":session,"configId":"autoRetry","value":"false"}),
        )
        .await;
    assert!(
        selected["result"]["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "autoRetry" && option["currentValue"] == "false")
    );
    let facts = peer
        .request(
            "_omp/rpc",
            json!({"sessionId":session,"command":"get_available_commands","arguments":{}}),
        )
        .await;
    assert_eq!(facts["result"]["commands"][0]["name"], "review");
    let forbidden = peer
        .request(
            "_omp/rpc",
            json!({"sessionId":session,"command":"prompt","arguments":{"message":"must use ACP"}}),
        )
        .await;
    assert!(forbidden.get("error").is_some());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let launches = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| {
            call["args"]
                .as_array()
                .is_some_and(|args| !args.contains(&json!("--no-session")))
        })
        .count();
    assert_eq!(
        launches, 1,
        "model, thinking and toggle changes must keep the same plugin process"
    );
    peer.finish(&session).await;
}

#[test]
fn native_error_events_are_redacted_without_rewriting_plugin_data() {
    let event = json!({"type":"arbitrary_extension_event","payload":{"custom":{"nested":[1,2,3]}}});
    assert_eq!(redact_event(&event), event);
    for event in [
        json!({"type":"prompt_result","error":"service unavailable api_key=fixture-secret"}),
        json!({"type":"message_end","message":{"role":"assistant","timestamp":1,"content":[],"stopReason":"error","errorMessage":"service unavailable api_key=fixture-secret"}}),
        json!({"type":"message_update","assistantMessageEvent":{"type":"error","message":"service unavailable api_key=fixture-secret"}}),
    ] {
        assert!(!redact_event(&event).to_string().contains("fixture-secret"));
    }
}

#[tokio::test]
async fn partial_acp_frames_survive_poll_ticks_and_keep_request_correlation() {
    let (_root, client, _spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    peer.input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"fragmented\",")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    peer.input.write_all(b"\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}\n").await.unwrap();
    let response = peer.next().await;
    assert_eq!(response["id"], "fragmented");
    assert_eq!(response["result"]["protocolVersion"], 1);
    peer.input.shutdown().await.unwrap();
    peer.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn native_controls_and_host_callbacks_remain_interactive_without_plugin_specific_routing() {
    let (root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, true).await;
    let control = peer
        .begin(
            "_omp/rpc",
            json!({"sessionId":session,"command":"interactive_control","arguments":{}}),
        )
        .await;
    loop {
        let message = peer.next().await;
        if message["method"] == "elicitation/create" {
            peer.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{"action":"accept","content":{"value":"native response"}}})).await;
        }
        if message.get("method").is_none() && message["id"] == control {
            assert_eq!(message["result"]["value"], "native response");
            break;
        }
    }
    let prompt = peer
        .begin(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"native-host"}]}),
        )
        .await;
    let mut unknown = false;
    loop {
        let message = peer.next().await;
        if message["method"] == "_omp/event" {
            let frame = &message["params"]["frame"];
            if frame["type"] == "future_native_event" {
                assert_eq!(frame["opaque"]["unmodified"], json!([1, 2, 3]));
                unknown = true;
            }
            if frame["type"] == "host_tool_call" {
                let rejected=peer.request("_omp/respond",json!({"sessionId":session,"frame":{"type":"host_tool_result","id":"wrong","result":{}}})).await;
                assert!(rejected.get("error").is_some());
                let reply=peer.request("_omp/respond",json!({"sessionId":session,
                    "frame":{"type":"host_tool_result","id":frame["id"],
                    "result":{"content":[{"type":"text","text":"host completed"}],"details":{"arbitrary-vendor-field":[7,8]}}}})).await;
                assert!(reply.get("result").is_some());
                break;
            }
        }
    }
    assert!(unknown);
    let result = peer.until(&prompt).await;
    assert_eq!(result.last().unwrap()["result"]["stopReason"], "end_turn");
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(
        calls
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|frame| frame["type"] == "host_tool_result"
                && frame["result"]["details"]["arbitrary-vendor-field"] == json!([7, 8]))
    );
    peer.finish(&session).await;
}

#[tokio::test]
async fn legacy_rpc_configuration_and_handles_use_acp_bridge_without_losing_identity() {
    let (_root, client, mut spec) = super::super::tests::fixture();
    spec.config.feature_values = Some(std::collections::BTreeMap::from([
        ("transport".to_owned(), json!("rpc")),
        ("autoRetry".to_owned(), json!(false)),
    ]));
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    super::super::tests::complete(session.as_mut()).await;
    let mut handle = session.persistence().unwrap();
    assert_eq!(handle.metadata.as_ref().unwrap()["transport"], "acp");
    assert_eq!(handle.metadata.as_ref().unwrap()["nativeTransport"], "rpc");
    session.close().await.unwrap();
    handle.metadata.as_mut().unwrap().remove("transport");
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn native_sdk_context_changes_refresh_identity_instead_of_failing_the_owner() {
    let (_root, client, spec) = super::super::tests::fixture();
    let mut native = super::super::session::open(&client, &spec, None)
        .await
        .unwrap();
    let before = native.persistence().unwrap().session_id;
    native
        .native_request("fixture_new_context", json!({}))
        .await
        .unwrap();
    let runtime = native.runtime_info().await.unwrap();
    assert_ne!(runtime.session_id.as_ref().unwrap(), &before);
    assert_eq!(
        Some(native.persistence().unwrap().session_id),
        runtime.session_id
    );
    native.close().await.unwrap();
}

#[tokio::test]
async fn standard_session_load_replays_native_user_and_assistant_history() {
    let (_root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client.clone());
    let session = peer.open(&spec.cwd, false).await;
    let result = peer
        .request(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"hello"}]}),
        )
        .await;
    assert_eq!(result["result"]["stopReason"], "end_turn");
    peer.finish(&session).await;
    let mut peer = Peer::new(client);
    peer.request(
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{}}),
    )
    .await;
    let id = peer
        .begin(
            "session/load",
            json!({"sessionId":session,"cwd":spec.cwd,"mcpServers":[]}),
        )
        .await;
    let messages = peer.until(&id).await;
    assert_eq!(messages.last().unwrap()["result"]["sessionId"], session);
    assert!(
        messages
            .iter()
            .any(|message| message["params"]["update"]["sessionUpdate"] == "user_message_chunk")
    );
    assert!(
        messages
            .iter()
            .any(|message| message["params"]["update"]["content"]["text"] == "Hello world")
    );
    peer.finish(&session).await;
}

#[tokio::test]
async fn standard_acp_failure_keeps_native_reason_http_status_and_redaction() {
    let (_root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, false).await;
    let response = peer
        .request(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"frame-error"}]}),
        )
        .await;
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Upstream shard unavailable")
    );
    assert_eq!(response["error"]["data"]["diagnostic"]["httpStatus"], 503);
    assert!(!response.to_string().contains("fixture-secret"));
    peer.finish(&session).await;
}

#[tokio::test]
async fn cli_bound_settings_keep_the_acp_identity_after_native_empty_session_replacement() {
    let (_root, client, spec) = super::super::tests::fixture();
    let mut peer = Peer::new(client.clone());
    let session = peer.open(&spec.cwd, true).await;
    let changed = peer
        .request(
            "session/set_mode",
            json!({"sessionId":session,"modeId":"ask"}),
        )
        .await;
    assert!(changed.get("result").is_some(), "{changed:?}");
    let state = peer
        .request(
            "_omp/rpc",
            json!({"sessionId":session,"command":"get_state"}),
        )
        .await;
    assert_ne!(state["result"]["sessionId"], session);
    let response = peer
        .request(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"hello"}]}),
        )
        .await;
    assert_eq!(response["result"]["stopReason"], "end_turn");
    peer.finish(&session).await;
    let mut peer = Peer::new(client);
    peer.request(
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{}}),
    )
    .await;
    let loaded = peer
        .request(
            "session/load",
            json!({"sessionId":session,"cwd":spec.cwd,"mcpServers":[]}),
        )
        .await;
    assert_eq!(loaded["result"]["sessionId"], session, "{loaded:?}");
    assert!(
        loaded["result"]["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "mode" && option["currentValue"] == "ask")
    );
    peer.finish(&session).await;
}

#[tokio::test]
async fn aliases_are_bounded_and_reject_corruption_cross_workspace_and_symlinks() {
    let (root, client, spec) = super::super::tests::fixture();
    let mut native = super::super::session::open(&client, &spec, None)
        .await
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let alias = aliases::Alias {
        cwd: spec.cwd.clone(),
        handle: native.persistence().unwrap(),
        config: spec.config.clone(),
    };
    assert!(aliases::load(&client, &id, &spec.cwd).unwrap().is_none());
    aliases::save(&client, &id, &alias).unwrap();
    assert_eq!(
        aliases::load(&client, &id, &spec.cwd)
            .unwrap()
            .unwrap()
            .handle
            .session_id,
        alias.handle.session_id
    );
    assert!(aliases::load(&client, &id, "/other-workspace").is_err());
    assert!(aliases::load(&client, "../invalid", &spec.cwd).is_err());
    let path = root
        .path()
        .join("agent/ait-acp-aliases")
        .join(format!("{id}.json"));
    std::fs::write(&path, "invalid JSON").unwrap();
    assert!(aliases::load(&client, &id, &spec.cwd).is_err());
    std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(aliases::load(&client, &id, &spec.cwd).is_err());
    #[cfg(unix)]
    {
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.path().join("elsewhere"), &path).unwrap();
        assert!(aliases::load(&client, &id, &spec.cwd).is_err());
    }
    native.close().await.unwrap();
}
