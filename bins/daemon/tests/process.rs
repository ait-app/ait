//! Real binary startup, environment handling, directory exclusion, and Unix signal shutdown.

use std::process::Command;

use async_trait as _;
use base64 as _;
use host_link as _;
#[cfg(unix)]
use libc as _;
// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use browser as _;
use clap as _;
use model as _;
use schedule as _;
use secrecy as _;
use serde as _;
use tokio_util as _;
use toml as _;
use tracing as _;
use tracing_subscriber as _;

#[test]
fn help_and_missing_credentials_do_not_create_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    let help = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .arg("--help")
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("--data-dir"));
    assert!(help_text.contains("Usage: daemon"));
    let version = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .arg("--version")
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        concat!("daemon ", env!("CARGO_PKG_VERSION"))
    );
    let failure = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args(["--data-dir", state.to_str().unwrap()])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("AIT_SERVER_TOKEN"));
    assert!(!state.exists());
}

#[cfg(unix)]
#[path = "process/unix.rs"]
mod unix;

#[test]
fn authorization_commands_are_explicit_and_status_never_requires_or_displays_secrets() {
    use std::io::Write;
    use std::process::Stdio;
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("absent");
    let status = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args(["status", "--json", "--data-dir", data.to_str().unwrap()])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(status.status.success());
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(json["center"], "https://dash.ait-app.com:8443/api");
    assert_eq!(json["credentials_present"], false);
    assert!(!data.exists());
    for token in ["", "bad token", "x\nsecret marker", &"x".repeat(8193)] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_daemon"))
            .args([
                "login",
                "--token-stdin",
                "--data-dir",
                data.to_str().unwrap(),
            ])
            .env_remove("AIT_SERVER_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(token.as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("secret marker"));
        assert!(!data.exists());
    }
}
