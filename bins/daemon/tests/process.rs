//! Real binary startup, environment handling, directory exclusion, and Unix signal shutdown.

use std::process::Command;

// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use browser as _;
use clap as _;
use file as _;
use model as _;
use schedule as _;
use tokio_util as _;
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
