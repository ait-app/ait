//! Bot permissions, derived subtree credentials and their rotation across a daemon restart,
//! observed through what the offline Codex receives and calls.

use std::path::{Path, PathBuf};
use std::time::Duration;

use domain::bot::BOT_SCOPE_LABEL;
use serde_json::{Value, json};

use super::mcp::Client;
use super::native::{NativeFixture, launches};
use super::transport::{Socket, connect, request};
use super::{Process, ready, start_with_path, terminate};

const PARENT_LABEL: &str = "paseo.parent-agent-id";

/// Calls written by one `ait-bot-probe` turn, after its `self` line.
const BOT_PROBE_CALLS: usize = 4;

/// Calls written by one `ait-scope-probe` turn, after its `self` line.
const SCOPE_PROBE_CALLS: usize = 4;

/// One running daemon with a host WebSocket and a host-token MCP client.
struct Daemon {
    process: Process,
    socket: Socket,
    client: Client,
}

impl Daemon {
    async fn start(fixture: &NativeFixture) -> Self {
        let state = fixture.root.path().join("state");
        let log = fixture.root.path().join("server.log");
        let mut process = start_with_path(&state, &log, Some(&fixture.path));
        let address = ready(&mut process, &log).await;
        let mut methods: Vec<_> = bot::capabilities::implemented_methods()
            .map(|method| method.name)
            .collect();
        methods.extend(["workspace.open.request", "schedule.run_once.request"]);
        Self {
            process,
            socket: connect(&address, &methods).await,
            client: Client::new(&address),
        }
    }

    async fn stop(mut self) {
        drop(self.socket);
        terminate(&mut self.process).await;
    }

    async fn success(&mut self, method: &str, params: Value) -> Value {
        let reply = request(&mut self.socket, method, params).await;
        assert_eq!(reply["type"], "response", "{method}: {reply}");
        reply["result"].clone()
    }

    async fn open_workspace(&mut self, cwd: &Path) -> String {
        let opened = self
            .success("workspace.open.request", json!({"cwd":cwd}))
            .await;
        opened["workspace"]["id"].as_str().unwrap().to_owned()
    }

    /// Send `text` as message `id` to the Bot of `workspace` and return the completed turn's
    /// reply.
    async fn bot_turn(&mut self, workspace: &str, id: &str, text: &str) -> Value {
        let opened = self
            .success(
                "bot.conversation.open.request",
                json!({"workspaceId":workspace}),
            )
            .await;
        let sent = self
            .success(
                "bot.message.send.request",
                json!({"workspaceId":workspace,"conversationId":opened["conversation"]["id"],
                    "messageId":id,"text":text}),
            )
            .await;
        assert_eq!(sent["accepted"], true, "{sent}");
        let turn = sent["turn"]["id"].clone();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let got = self
                    .success(
                        "bot.turn.get.request",
                        json!({"workspaceId":workspace,"turnId":turn}),
                    )
                    .await;
                let status = got["turn"]["status"].as_str().unwrap().to_owned();
                assert!(!matches!(status.as_str(), "failed" | "uncertain"), "{got}");
                if status == "completed" {
                    break got["turn"]["reply"].clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    /// Prompt Agent `id` with `text` through the host token and return its last message.
    async fn prompt(&self, id: &str, text: &str) -> Value {
        self.client
            .tool(
                "send_agent_prompt",
                json!({"agentId":id,"prompt":text}),
                None,
            )
            .await;
        self.client
            .tool("wait_for_agent", json!({"agentId":id}), None)
            .await["lastMessage"]
            .clone()
    }

    async fn snapshot(&self, id: &str) -> Value {
        self.client
            .tool("get_agent_status", json!({"agentId":id}), None)
            .await["snapshot"]
            .clone()
    }

    /// HTTP status of a bare `tools/list` presented with `authorization`.
    async fn status_with(&self, authorization: &str) -> reqwest::StatusCode {
        self.client
            .http
            .post(&self.client.url)
            .header("authorization", authorization)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":"credential","method":"tools/list","params":{}}))
            .send()
            .await
            .unwrap()
            .status()
    }
}

/// A Bot installed over offline Codex that has run one `ait-bot-probe` turn.
struct Deployment {
    fixture: NativeFixture,
    daemon: Daemon,
    workspace: String,
    /// Agent in an unrelated Workspace, created by the user with the shared MCP token.
    outsider: String,
    /// `Authorization` header injected into ordinary user Agents.
    shared: String,
    /// The Bot-probe's `self` line followed by one response per call.
    bot_probe: Vec<Value>,
    /// The Agent the Bot created within its permissions.
    child: String,
}

impl Deployment {
    async fn start() -> Self {
        let fixture = NativeFixture::new();
        let mut daemon = Daemon::start(&fixture).await;
        let workspace = daemon.open_workspace(&fixture.cwd).await;
        let elsewhere = fixture.root.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let other = daemon.open_workspace(&elsewhere).await;
        let created = daemon
            .client
            .tool(
                "create_agent",
                json!({"provider":"codex/offline-model","workspaceId":other,
                    "title":"Outsider","initialPrompt":"ready"}),
                None,
            )
            .await;
        let outsider = created["agentId"].as_str().unwrap().to_owned();
        daemon
            .client
            .tool("wait_for_agent", json!({"agentId":outsider}), None)
            .await;
        let shared = authorization(&launches(&elsewhere, &outsider)[0]);
        daemon
            .success(
                "bot.setup.request",
                json!({"workspaceId":workspace,"execution":{"provider":"codex"},
                    "permissions":{"groups":["agents","schedules"],"modes":{"codex":["auto"]}}}),
            )
            .await;
        assert_eq!(
            daemon
                .bot_turn(&workspace, "probe-before-restart", "ait-bot-probe")
                .await,
            "Echo: bot-probe-ok"
        );
        let bot_probe = probe_lines(&fixture.cwd, "bot-probe");
        assert_eq!(bot_probe.len(), 1 + BOT_PROBE_CALLS, "{bot_probe:?}");
        let child = structured(&bot_probe[3])["agentId"]
            .as_str()
            .unwrap_or_else(|| panic!("{}", bot_probe[3]))
            .to_owned();
        daemon
            .client
            .tool("wait_for_agent", json!({"agentId":child}), None)
            .await;
        Self {
            fixture,
            daemon,
            workspace,
            outsider,
            shared,
            bot_probe,
            child,
        }
    }

    /// Stop the daemon and start a new one over the same state directory.
    async fn restart(mut self) -> Self {
        self.daemon.stop().await;
        self.daemon = Daemon::start(&self.fixture).await;
        self
    }

    fn state(&self) -> PathBuf {
        self.fixture.root.path().join("state")
    }

    fn log(&self) -> PathBuf {
        self.fixture.root.path().join("server.log")
    }

    /// Run `ait-scope-probe` on the Bot's child and return its newest `self` and responses.
    async fn scope_probe(&self) -> Vec<Value> {
        assert_eq!(
            self.daemon.prompt(&self.child, "ait-scope-probe").await,
            "Echo: scope-probe-ok"
        );
        let lines = probe_lines(&self.fixture.cwd, "scope-probe");
        assert_eq!(lines.len() % (1 + SCOPE_PROBE_CALLS), 0, "{lines:?}");
        lines[lines.len() - 1 - SCOPE_PROBE_CALLS..].to_vec()
    }

    /// Assert that the scope-probe's denials and the Workspace-confined listing hold.
    fn assert_confined(&self, probe: &[Value]) {
        assert_eq!(probe[0]["self"]["agentId"], self.child.as_str());
        for refused in &probe[1..3] {
            assert_eq!(refused["error"]["code"], -32602, "{refused}");
            assert_eq!(
                refused["error"]["message"], "Unknown or unavailable tool",
                "{refused}"
            );
        }
        forbidden(&probe[3]);
        let listed = structured(&probe[4])["agents"].as_array().unwrap().clone();
        assert!(
            listed
                .iter()
                .any(|agent| agent["id"] == self.child.as_str()),
            "{listed:?}"
        );
        assert!(
            listed
                .iter()
                .all(|agent| agent["workspaceId"] == self.workspace.as_str()
                    && agent["id"] != self.outsider.as_str()),
            "{listed:?}"
        );
    }

    /// Assert that no `credentials` occur in any daemon state file or the server log.
    fn assert_private(&self, credentials: &[&str]) {
        let mut files = vec![self.log()];
        collect_files(&self.state(), &mut files);
        assert!(
            files
                .iter()
                .any(|path| path.ends_with("agents/agents.json"))
        );
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            for credential in credentials {
                let secret = credential.trim_start_matches("Bearer ").as_bytes();
                assert!(
                    !bytes.windows(secret.len()).any(|window| window == secret),
                    "credential leaked into {}",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn bot_manages_agents_within_permissions_and_child_inherits_scope() {
    let mut deployment = Deployment::start().await;
    let bot = deployment.bot_probe[0]["self"].clone();
    let bot_id = bot["agentId"].as_str().unwrap();
    let bot_authorization = bot["authorization"].as_str().unwrap();

    // The Bot's own hidden session: read-only sandbox and a Bot credential.
    let bot_launches = launches(&deployment.fixture.cwd, bot_id);
    let start = bot_launches
        .iter()
        .find(|launch| launch["method"] == "thread/start")
        .unwrap_or_else(|| panic!("{bot_launches:?}"));
    assert_eq!(start["params"]["sandbox"], "read-only", "{start}");
    assert_eq!(authorization(start), bot_authorization);
    derived(bot_authorization);
    assert_ne!(bot_authorization, deployment.shared);
    assert_ne!(bot_authorization, format!("Bearer {}", super::TOKEN));
    let listed = deployment
        .daemon
        .client
        .tool("list_agents", json!({"includeArchived":true}), None)
        .await;
    assert!(
        listed["agents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|agent| agent["id"] != bot_id),
        "{listed}"
    );

    // What the Bot could see and do through its credential.
    let tools = tool_names(&deployment.bot_probe[1]);
    for hidden in ["respond_to_permission", "archive_workspace"] {
        assert!(!tools.contains(&hidden.to_owned()), "{tools:?}");
    }
    for granted in ["create_agent", "create_schedule", "bot_memory_read"] {
        assert!(tools.contains(&granted.to_owned()), "{tools:?}");
    }
    forbidden(&deployment.bot_probe[2]);

    // The child inherits the Bot scope, not parentage, and starts in the cap's default mode.
    let child = deployment.child.clone();
    let snapshot = deployment.daemon.snapshot(&child).await;
    assert_eq!(
        snapshot["labels"][BOT_SCOPE_LABEL],
        deployment.workspace.as_str()
    );
    assert!(snapshot["labels"].get(PARENT_LABEL).is_none(), "{snapshot}");
    assert_eq!(snapshot["workspaceId"], deployment.workspace.as_str());
    assert_eq!(snapshot["currentModeId"], "auto", "{snapshot}");
    let child_launch = &launches(&deployment.fixture.cwd, &child)[0];
    let child_authorization = authorization(child_launch);
    assert_eq!(child_launch["params"]["sandbox"], "workspace-write");
    derived(&child_authorization);
    assert_ne!(child_authorization, deployment.shared);
    assert_ne!(child_authorization, bot_authorization);

    // The subtree credential is confined: HostOnly tools are absent, the cap holds, and
    // listings stay inside the Bot's Workspace.
    let probe = deployment.scope_probe().await;
    assert_eq!(probe[0]["self"]["authorization"], child_authorization);
    deployment.assert_confined(&probe);

    // A schedule the Bot created runs as a Bot-scoped Agent with its own derived credential.
    let schedule = structured(&deployment.bot_probe[4])["schedule"]["id"].clone();
    let ran = deployment
        .daemon
        .success("schedule.run_once.request", json!({"scheduleId":schedule}))
        .await;
    assert_eq!(ran["schedule"]["runs"][0]["status"], "succeeded", "{ran}");
    let scheduled = ran["schedule"]["runs"][0]["agentId"]
        .as_str()
        .unwrap()
        .to_owned();
    let snapshot = deployment.daemon.snapshot(&scheduled).await;
    assert_eq!(
        snapshot["labels"][BOT_SCOPE_LABEL],
        deployment.workspace.as_str()
    );
    let scheduled_authorization = authorization(&launches(&deployment.fixture.cwd, &scheduled)[0]);
    derived(&scheduled_authorization);
    for other in [&deployment.shared, &child_authorization, bot_authorization] {
        assert_ne!(&scheduled_authorization, other);
    }

    deployment.assert_private(&[
        bot_authorization,
        &child_authorization,
        &scheduled_authorization,
    ]);
    deployment.daemon.stop().await;
}

#[tokio::test]
async fn bot_scope_survives_daemon_restart() {
    let deployment = Deployment::start().await;
    let before = deployment.scope_probe().await;
    deployment.assert_confined(&before);
    let old_child = before[0]["self"]["authorization"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_bot = deployment.bot_probe[0]["self"]["authorization"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        deployment.daemon.status_with(&old_child).await,
        reqwest::StatusCode::OK
    );

    // Each start truncates the log, so check this run's log before restarting.
    deployment.assert_private(&[&old_child, &old_bot]);
    let mut deployment = deployment.restart().await;

    for stale in [&old_child, &old_bot] {
        assert_eq!(
            deployment.daemon.status_with(stale).await,
            reqwest::StatusCode::UNAUTHORIZED
        );
    }
    let after = deployment.scope_probe().await;
    let renewed = after[0]["self"]["authorization"]
        .as_str()
        .unwrap()
        .to_owned();
    derived(&renewed);
    assert_ne!(renewed, old_child);
    assert_ne!(renewed, deployment.shared);
    assert_eq!(
        authorization(
            launches(&deployment.fixture.cwd, &deployment.child)
                .last()
                .unwrap()
        ),
        renewed
    );
    deployment.assert_confined(&after);
    for (old, new) in before[1..4].iter().zip(&after[1..4]) {
        assert_eq!(old["error"], new["error"]);
        assert_eq!(old["result"]["isError"], new["result"]["isError"]);
    }

    // The Bot's next turn receives a fresh Bot credential that the restarted daemon accepts.
    assert_eq!(
        deployment
            .daemon
            .bot_turn(
                &deployment.workspace,
                "probe-after-restart",
                "ait-bot-probe"
            )
            .await,
        "Echo: bot-probe-ok"
    );
    let probes = probe_lines(&deployment.fixture.cwd, "bot-probe");
    let fresh = &probes[probes.len() - 1 - BOT_PROBE_CALLS..];
    let fresh_bot = fresh[0]["self"]["authorization"].as_str().unwrap();
    derived(fresh_bot);
    assert_ne!(fresh_bot, old_bot);
    assert!(tool_names(&fresh[1]).contains(&"create_agent".to_owned()));
    deployment.assert_private(&[&old_child, &old_bot, &renewed, fresh_bot]);
    deployment.daemon.stop().await;
}

/// The `Authorization` header injected into a native launch request.
fn authorization(launch: &Value) -> String {
    launch["params"]["config"]["mcp_servers"]["ait"]["http_headers"]["Authorization"]
        .as_str()
        .unwrap_or_else(|| panic!("{launch}"))
        .to_owned()
}

/// Assert that `authorization` carries a daemon-derived scoped credential: 64 hex characters,
/// unlike the shared Agent MCP token.
fn derived(authorization: &str) {
    let secret = authorization
        .strip_prefix("Bearer ")
        .unwrap_or_else(|| panic!("{authorization}"));
    assert!(
        secret.len() == 64 && secret.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{authorization}"
    );
}

/// Assert that a recorded tool call was refused by the Bot's scope or mode cap.
fn forbidden(response: &Value) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["content"][0]["text"],
        model::ErrorCode::Forbidden.message(),
        "{response}"
    );
}

/// Lines of `<cwd>/<name>.jsonl` written by the offline Codex probes.
fn probe_lines(cwd: &Path, name: &str) -> Vec<Value> {
    std::fs::read_to_string(cwd.join(format!("{name}.jsonl")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The successful structured result of a recorded `tools/call` response.
fn structured(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], false, "{response}");
    &response["result"]["structuredContent"]
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{response}"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}
