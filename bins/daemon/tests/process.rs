//! Real binary startup, environment handling, directory exclusion, and Unix signal shutdown.

use std::process::Command;

use async_trait as _;
use base64 as _;
use host_link as _;
#[cfg(unix)]
use libc as _;
#[cfg(unix)]
use rustix as _;
// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use bot as _;
use browser as _;
use clap as _;
use jsonwebtoken as _;
use model as _;
use persistence as _;
use provider_builtin as _;
use ring as _;
use schedule as _;
use secrecy as _;
use serde as _;
use thiserror as _;
use tokio_util as _;
use toml as _;
use tracing as _;
use tracing_subscriber as _;

#[test]
fn help_and_invalid_credentials_do_not_create_state() {
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
    assert!(help_text.contains("Usage: aitd [OPTIONS] [COMMAND]"));
    let subcommand_help = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args(["login", "--help"])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(subcommand_help.status.success());
    assert!(String::from_utf8_lossy(&subcommand_help.stdout).contains("Usage: aitd login"));
    let version = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .arg("--version")
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("aitd {}", model::BUILD_VERSION)
    );
    let failure = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args(["--data-dir", state.to_str().unwrap()])
        .env("AIT_SERVER_TOKEN", "invalid-token")
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
    assert_eq!(json["center"], "https://aitapp.cn/api");
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "process/local_token.rs"]
mod local_token;

#[test]
fn occupied_port_does_not_create_credentials_or_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("absent");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let result = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args([
            "--data-dir",
            directory.to_str().unwrap(),
            "--listen",
            &address,
        ])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("bind server listener"));
    assert!(!directory.exists());
}

#[test]
fn owned_data_directory_is_not_modified_before_lock_acquisition() {
    let root = tempfile::tempdir().unwrap();
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.path().join("instance.lock"))
        .unwrap();
    lease.try_lock().unwrap();
    let diagnostics = root.path().join("diagnostics");
    std::fs::create_dir(&diagnostics).unwrap();
    let incidents: Vec<_> = (0..21)
        .map(|_| diagnostics.join(format!("incident-{}.txt", uuid::Uuid::new_v4())))
        .collect();
    for incident in &incidents {
        std::fs::write(incident, "preserve the running daemon's evidence").unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args([
            "--data-dir",
            root.path().to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already in use"));
    assert!(!root.path().join("local-api.token").exists());
    assert!(incidents.iter().all(|path| path.exists()));
    assert_eq!(std::fs::read_dir(&diagnostics).unwrap().count(), 21);
}
