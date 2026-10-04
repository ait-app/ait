//! Every child process is started without the server's credentials (ADR-083).
//!
//! The server cannot delete `BONSAI_RUNTIME_*` or `AIT_SERVER_*` from its own environment, so
//! each spawn path removes them with `model::process::private_environment()`. This guard
//! pins the inventory of process constructors: a new constructor fails here until its path
//! strips the credentials and the table below records it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use axum as _;
use bonsai as _;
use browser as _;
use chrono as _;
use clap as _;
use domain as _;
use filesystem as _;
use futures_util as _;
use metadata as _;
use model as _;
use protocol as _;
use provider as _;
use reqwest as _;
use schedule as _;
use secrecy as _;
use serde as _;
use serde_json as _;
use tempfile as _;
use terminal as _;
use tokio as _;
use tokio_tungstenite as _;
use tokio_util as _;
use toml as _;
use tracing as _;
use tracing_subscriber as _;
use uuid as _;
use voice as _;

const STRIP: &str = "model::process::private_environment()";

/// Source file, number of process constructors in it, and the file whose spawn path strips
/// the credentials for those constructors. Constructors that only signal a process (`/bin/kill`,
/// `taskkill`) run no foreign code and are counted with the file that owns them.
const SPAWN_SITES: &[(&str, usize, &str)] = &[
    (
        "crates/bonsai/src/hello.rs",
        1,
        "crates/bonsai/src/hello.rs",
    ),
    (
        "crates/filesystem/src/local/checkout.rs",
        1,
        "crates/filesystem/src/local/checkout.rs",
    ),
    (
        "crates/filesystem/src/local/forge.rs",
        2,
        "crates/filesystem/src/local/forge.rs",
    ),
    (
        "crates/filesystem/src/local/forge/gitlab.rs",
        1,
        "crates/filesystem/src/local/forge.rs",
    ),
    (
        "crates/filesystem/src/local/forge/resolver.rs",
        2,
        "crates/filesystem/src/local/forge.rs",
    ),
    (
        "crates/filesystem/src/local/git.rs",
        1,
        "crates/filesystem/src/local/git.rs",
    ),
    (
        "crates/filesystem/src/local/git_fetch.rs",
        3,
        "crates/filesystem/src/local/git_fetch.rs",
    ),
    (
        "crates/filesystem/src/local/github_projects.rs",
        2,
        "crates/filesystem/src/local/forge.rs",
    ),
    (
        "crates/filesystem/src/local/worktrees.rs",
        1,
        "crates/filesystem/src/local/worktrees.rs",
    ),
    (
        "crates/metadata/src/local/workspace_automation.rs",
        5,
        "crates/metadata/src/local/workspace_automation.rs",
    ),
    (
        "crates/provider/src/local/claude/inspection.rs",
        2,
        "crates/provider/src/local/claude/inspection.rs",
    ),
    (
        "crates/provider/src/local/claude/transport.rs",
        3,
        "crates/provider/src/local/claude/transport.rs",
    ),
    (
        "crates/provider/src/local/codex/transport.rs",
        3,
        "crates/provider/src/local/codex/transport.rs",
    ),
    (
        "crates/provider/src/local/deepseek_harness/transport.rs",
        2,
        "crates/provider/src/local/deepseek_harness/transport.rs",
    ),
    (
        "crates/provider/src/local/opencode/runtime.rs",
        4,
        "crates/provider/src/local/opencode/runtime.rs",
    ),
    (
        "crates/terminal/src/local.rs",
        1,
        "crates/terminal/src/local.rs",
    ),
    ("crates/voice/src/local.rs", 2, "crates/voice/src/local.rs"),
    (
        "crates/voice/src/offline/worker.rs",
        1,
        "crates/voice/src/offline/worker.rs",
    ),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn is_test_source(relative: &str) -> bool {
    relative.contains("/tests/") || relative.ends_with("/tests.rs")
}

fn sources(directory: &Path, root: &Path, found: &mut Vec<(String, String)>) {
    let entries = std::fs::read_dir(directory).expect("read source directory");
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            sources(&path, root, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root)
                .expect("source below the workspace root")
                .to_string_lossy()
                .replace('\\', "/");
            if !is_test_source(&relative) {
                let text = std::fs::read_to_string(&path).expect("read source file");
                found.push((relative, text));
            }
        }
    }
}

fn constructors(text: &str) -> usize {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
        .map(|line| {
            line.matches("Command::new(").count() + line.matches("CommandBuilder::new(").count()
        })
        .sum()
}

fn production_sources() -> Vec<(String, String)> {
    let root = workspace_root()
        .canonicalize()
        .expect("resolve workspace root");
    let mut found = Vec::new();
    for top in ["crates", "bins"] {
        for package in std::fs::read_dir(root.join(top)).expect("list packages") {
            let source = package.expect("read package entry").path().join("src");
            if source.is_dir() {
                sources(&source, &root, &mut found);
            }
        }
    }
    found
}

#[test]
fn every_process_constructor_is_inventoried() {
    let actual: BTreeMap<String, usize> = production_sources()
        .into_iter()
        .filter_map(|(path, text)| {
            let count = constructors(&text);
            (count > 0).then_some((path, count))
        })
        .collect();
    let expected: BTreeMap<String, usize> = SPAWN_SITES
        .iter()
        .map(|(path, count, _)| ((*path).to_owned(), *count))
        .collect();
    assert_eq!(
        actual, expected,
        "process constructors changed: strip {STRIP} on the new path and update SPAWN_SITES"
    );
}

#[test]
fn every_spawn_path_strips_server_credentials() {
    let root = workspace_root();
    for (site, _, strip_file) in SPAWN_SITES {
        let text = std::fs::read_to_string(root.join(strip_file)).expect("read strip file");
        assert!(
            text.contains(STRIP),
            "{site}: {strip_file} does not call {STRIP}"
        );
    }
}

#[test]
fn comments_and_test_sources_are_not_counted() {
    assert_eq!(
        constructors("// Command::new(\"x\")\nlet c = Command::new(\"y\");"),
        1
    );
    assert_eq!(constructors("CommandBuilder::new(shell)"), 1);
    assert!(is_test_source("crates/a/src/b/tests.rs"));
    assert!(is_test_source("crates/a/src/b/tests/c.rs"));
    assert!(!is_test_source("crates/a/src/b/latest.rs"));
}
