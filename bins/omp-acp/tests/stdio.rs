//! CLI process boundary tests; no installed OMP or model credentials are required.

use std::process::{Output, Stdio};
use std::time::Duration;

use clap as _;
use provider as _;
use serde_json::Value;
use tempfile as _;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing as _;
use tracing_subscriber as _;

async fn run(arguments: &[&str], input: &[u8]) -> Output {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ait-omp-acp"))
            .args(arguments)
            .env_remove("AIT_SERVER_OMP_BIN")
            .env_remove("AIT_SERVER_DATA_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input).await.unwrap();
        stdin.shutdown().await.unwrap();
        drop(stdin);
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("CLI must exit promptly after stdin closes")
}

#[tokio::test]
async fn version_and_help_succeed_without_runtime_or_daemon_configuration() {
    let version = run(&["--version"], b"").await;
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!("ait-omp-acp {}\n", model::BUILD_VERSION)
    );
    assert!(version.stderr.is_empty());
    let help = run(&["--help"], b"").await;
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--omp-bin")
    );
    assert!(help.stderr.is_empty());
}

#[tokio::test]
async fn invalid_cli_exits_with_diagnostics_only_on_stderr() {
    let output = run(&["--unknown"], b"").await;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[tokio::test]
async fn empty_stdin_exits_successfully_without_launching_omp() {
    let output = run(&["--omp-bin", "/nonexistent OMP executable/omp"], b"").await;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn initialize_is_jsonl_and_eof_exits_successfully() {
    let output = run(
        &["--omp-bin", "/nonexistent OMP executable/omp"],
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}\n",
    )
    .await;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let frame = output.stdout.strip_suffix(b"\n").expect("JSONL terminator");
    assert!(!frame.contains(&b'\n'));
    let response: Value = serde_json::from_slice(frame).unwrap();
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], 1);
    assert_eq!(response["result"]["agentInfo"]["name"], "ait-omp-acp");
    assert_eq!(
        response["result"]["agentInfo"]["version"],
        model::BUILD_VERSION
    );
}

#[tokio::test]
async fn malformed_json_is_a_protocol_error_not_a_process_failure() {
    let output = run(&[], b"not JSON\n").await;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], -32700);
}

#[tokio::test]
async fn truncated_frame_fails_with_stderr_only_diagnostics() {
    let output = run(&[], b"{\"jsonrpc\":\"2.0\"").await;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("OMP ACP bridge stopped"));
    assert!(diagnostic.contains("Invalid or oversized ACP frame"));
}
