//! Real user-installed OMP, isolated public extension and no model/network inference.
#![cfg(unix)]

use clap as _;
use provider as _;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tracing as _;
use tracing_subscriber as _;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; checks ACP stdio, EOF and native descendant cleanup"]
async fn installed_omp_stdio_eof_reaps_the_native_process_group() {
    let (root, agent, home) = fixture();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let mut bridge = Command::new(env!("CARGO_BIN_EXE_ait-omp-acp"))
        .arg("--omp-bin")
        .arg(program)
        .env("HOME", &home)
        .env("PI_CODING_AGENT_DIR", &agent)
        .env("OMP_PROFILE", "")
        .env("PI_PROFILE", "")
        .env(
            "OMP_BOUNDARY_CHILD_RECORD",
            root.path().join("boundary-child.json"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = bridge.stdin.take().unwrap();
    let mut output = BufReader::new(bridge.stdout.take().unwrap());
    let initialized = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":1,"clientCapabilities":{}}
        }),
    )
    .await;
    assert_eq!(initialized["agentInfo"]["version"], model::BUILD_VERSION);
    let created = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0","id":2,"method":"session/new",
            "params":{"cwd":root.path(),"mcpServers":[]}
        }),
    )
    .await;
    let session_id = created["sessionId"].as_str().unwrap();
    let prompt = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0","id":3,"method":"session/prompt",
            "params":{"sessionId":session_id,"prompt":[{"type":"text","text":"/boundary-child"}]}
        }),
    )
    .await;
    assert_eq!(prompt["stopReason"], "end_turn");
    let pids: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("boundary-child.json")).unwrap())
            .unwrap();
    let native = pids["native"].as_u64().unwrap();
    let child = pids["child"].as_u64().unwrap();
    assert!(alive(native));
    assert!(alive(child));

    input.shutdown().await.unwrap();
    drop(input);
    let mut remainder = Vec::new();
    let (drained, exited) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(
            output.read_to_end(&mut remainder),
            bridge.wait_with_output()
        )
    })
    .await
    .expect("stdin EOF must close the native session and bridge promptly");
    drained.unwrap();
    let exited = exited.unwrap();
    assert!(
        exited.status.success(),
        "{}",
        String::from_utf8_lossy(&exited.stderr)
    );
    for line in remainder
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let frame: Value = serde_json::from_slice(line).expect("stdout must remain JSONL");
        assert_eq!(frame["jsonrpc"], "2.0");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while alive(native) || alive(child) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("EOF must terminate the native process and its descendant");
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let agent = root.path().join("agent");
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(agent.join("extensions")).unwrap();
    std::fs::write(
        agent.join("extensions/boundary-extension.ts"),
        include_str!("../../../crates/provider/src/local/omp/tests/fixtures/boundary-extension.ts"),
    )
    .unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
            "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
            "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
                "contextWindow":32768,"maxTokens":1024,
                "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]
        }}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        agent.join("config.yml"),
        "modelRoles:\n  default: ait-local-fixture/fixture\n",
    )
    .unwrap();
    (root, agent, home)
}

async fn request(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request: Value,
) -> Value {
    input
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let mut line = String::new();
            assert_ne!(
                output.read_line(&mut line).await.unwrap(),
                0,
                "unexpected stdout EOF"
            );
            let frame: Value = serde_json::from_str(&line)
                .expect("stdout must contain only JSONL protocol frames");
            assert_eq!(frame["jsonrpc"], "2.0");
            if frame["id"] == request["id"] {
                assert!(frame.get("error").is_none(), "{frame}");
                return frame["result"].clone();
            }
        }
    })
    .await
    .expect("native ACP request timed out")
}

#[cfg(target_os = "linux")]
fn alive(pid: u64) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| !fields.starts_with('Z'))
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn alive(pid: u64) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
