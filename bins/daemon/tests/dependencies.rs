//! Architectural regression guard, including optional, target-specific, dev and build edges.

use std::collections::BTreeSet;
use std::process::Command;

// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use axum as _;
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

use serde_json::{Value, json};

fn violations(packages: &[Value]) -> Vec<String> {
    let workspace_names: BTreeSet<_> = packages
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    let mut violations = Vec::new();
    for package in packages {
        let name = package["name"].as_str().unwrap();
        let allowed: &[&str] = match name {
            "daemon" => &[
                "voice",
                "schedule",
                "browser",
                "model",
                "metadata",
                "filesystem",
                "provider",
                "api",
                "terminal",
                "protocol",
                "domain",
            ],
            "api" => &[
                "relay",
                "voice",
                "schedule",
                "browser",
                "model",
                "terminal",
                "provider",
                "protocol",
                "metadata",
                "filesystem",
            ],
            "provider" => &["domain", "model"],
            "protocol" | "metadata" | "voice" | "schedule" | "browser" | "terminal" => &["model"],
            "filesystem" => &["metadata", "model"],
            "domain" | "model" | "relay" => &[],
            _ => {
                violations.push(format!("unregistered workspace package: {name}"));
                continue;
            }
        };
        for dependency in package["dependencies"].as_array().unwrap() {
            let target = dependency["name"].as_str().unwrap();
            if (workspace_names.contains(target) || dependency["path"].is_string())
                && !allowed.contains(&target)
            {
                violations.push(format!("{name} -> {target}"));
            }
            if matches!(
                name,
                "domain" | "model" | "protocol" | "metadata" | "filesystem" | "terminal"
            ) && [
                "sqlx", "rusqlite", "axum", "hyper", "reqwest", "tonic", "tauri", "rig", "codex",
            ]
            .iter()
            .any(|prefix| target == *prefix || target.starts_with(&format!("{prefix}-")))
            {
                violations.push(format!("impure {name} -> {target}"));
            }
            if matches!(name, "domain" | "protocol")
                && (target == "tokio" || target.starts_with("tokio-"))
            {
                violations.push(format!("impure {name} -> {target}"));
            }
        }
    }
    violations
}

#[test]
fn all_workspace_dependency_edges_follow_current_adrs() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let violations = violations(metadata["packages"].as_array().unwrap());
    assert!(
        violations.is_empty(),
        "workspace boundary violations: {violations:?}"
    );
}

#[test]
fn guard_catches_indirect_renamed_optional_target_and_development_edges() {
    for kind in [Value::Null, json!("dev"), json!("build")] {
        let packages = vec![
            json!({"id":"daemon", "name":"daemon", "dependencies":[{"name":"api", "path":"../api"}]}),
            json!({"id":"api", "name":"api", "dependencies":[{"name":"ait-domain", "rename":"innocent", "kind":kind, "optional":true, "target":"cfg(windows)"}]}),
            json!({"id":"ait-domain", "name":"ait-domain", "dependencies":[]}),
        ];
        assert_eq!(
            violations(&packages),
            [
                "api -> ait-domain",
                "unregistered workspace package: ait-domain"
            ]
        );
    }
    assert_eq!(
        violations(&[json!({"id":"domain", "name":"domain", "dependencies":[{"name":"tokio"}]})]),
        ["impure domain -> tokio"]
    );
}

#[test]
fn metadata_cannot_depend_on_host_crates_or_transport_adapters() {
    for dependency in [
        "protocol",
        "provider",
        "domain",
        "ports",
        "application",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"metadata", "name":"metadata", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("metadata -> {dependency}")]);
    }
    for dependency in ["rusqlite", "axum", "reqwest"] {
        let packages =
            [json!({"id":"metadata", "name":"metadata", "dependencies":[{"name":dependency}]})];
        assert_eq!(
            violations(&packages),
            [format!("impure metadata -> {dependency}")]
        );
    }
}

#[test]
fn filesystem_cannot_depend_on_host_agent_or_transport_crates() {
    for dependency in [
        "api",
        "protocol",
        "application",
        "ports",
        "domain",
        "workspace",
        "provider",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"filesystem", "name":"filesystem", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(
            violations(&packages),
            [format!("filesystem -> {dependency}")]
        );
    }
    for dependency in ["axum", "reqwest", "rusqlite"] {
        let packages =
            [json!({"id":"filesystem", "name":"filesystem", "dependencies":[{"name":dependency}]})];
        assert_eq!(
            violations(&packages),
            [format!("impure filesystem -> {dependency}")]
        );
    }
}

#[test]
fn provider_depends_inward_and_retired_packages_cannot_return() {
    for dependency in ["api", "protocol", "filesystem", "metadata", "ait-domain"] {
        let packages = [
            json!({"id":"provider", "name":"provider", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("provider -> {dependency}")]);
    }
    for retired in ["application", "ports", "storage", "workspace", "providers"] {
        let packages = [json!({"id":retired, "name":retired, "dependencies":[]})];
        assert_eq!(
            violations(&packages),
            [format!("unregistered workspace package: {retired}")]
        );
    }
}

#[test]
fn terminal_cannot_depend_on_metadata_provider_transport_or_old_workspace_packages() {
    for dependency in [
        "api",
        "protocol",
        "provider",
        "domain",
        "filesystem",
        "metadata",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"terminal", "name":"terminal", "dependencies":[{"name":dependency,"path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("terminal -> {dependency}")]);
    }
    for kind in [Value::Null, json!("dev"), json!("build")] {
        let packages = [
            json!({"id":"terminal", "name":"terminal", "dependencies":[{"name":"metadata", "rename":"workspace_records", "kind":kind, "optional":true, "target":"cfg(windows)", "path":"../metadata"}]}),
        ];
        assert_eq!(violations(&packages), ["terminal -> metadata"]);
    }
    for dependency in ["axum", "rusqlite", "reqwest"] {
        let packages =
            [json!({"id":"terminal", "name":"terminal", "dependencies":[{"name":dependency}]})];
        assert_eq!(
            violations(&packages),
            [format!("impure terminal -> {dependency}")]
        );
    }
}

#[test]
fn tokio_is_allowed_in_capability_crates_but_not_domain_or_protocol() {
    for name in [
        "model",
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "domain",
        "protocol",
    ] {
        for dependency in ["tokio", "tokio-util"] {
            for kind in [Value::Null, json!("dev"), json!("build")] {
                let packages = [
                    json!({"id":name, "name":name, "dependencies":[{"name":dependency,"kind":kind,"optional":true,"target":"cfg(windows)"}]}),
                ];
                let expected = if matches!(name, "domain" | "protocol") {
                    vec![format!("impure {name} -> {dependency}")]
                } else {
                    Vec::new()
                };
                assert_eq!(violations(&packages), expected);
            }
        }
    }
}

#[test]
fn shared_context_cannot_depend_on_capability_or_transport_packages() {
    for dependency in [
        "api",
        "protocol",
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "domain",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"model", "name":"model", "dependencies":[{"name":dependency,"path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("model -> {dependency}")]);
    }
}

#[test]
fn protocol_cannot_depend_on_business_crates_even_through_test_or_optional_edges() {
    for dependency in [
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "voice",
        "domain",
        "api",
        "ait-domain",
    ] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"id":"protocol", "name":"protocol", "dependencies":[{
                "name":dependency, "path":"../dependency", "rename":"renamed",
                "kind":kind, "optional":true, "target":"cfg(windows)"
            }]})];
            assert_eq!(violations(&packages), [format!("protocol -> {dependency}")]);
        }
    }
}

#[test]
fn voice_keeps_speech_io_but_cannot_depend_on_transport_or_agent_implementation() {
    for dependency in ["api", "protocol", "provider", "metadata", "ait-domain"] {
        let packages = [
            json!({"id":"voice", "name":"voice", "dependencies":[{"name":dependency,"path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("voice -> {dependency}")]);
    }
    let packages = [
        json!({"id":"voice", "name":"voice", "dependencies":[{"name":"model","path":"../model"},{"name":"reqwest"},{"name":"tokio"}]}),
    ];
    assert!(violations(&packages).is_empty());
}

#[test]
fn relay_is_a_leaf_transport_owned_by_the_api() {
    let packages = [
        json!({"id":"api", "name":"api", "dependencies":[{"name":"relay", "path":"../relay"}]}),
        json!({"id":"relay", "name":"relay", "dependencies":[{"name":"tokio"}, {"name":"reqwest"}]}),
    ];
    assert!(violations(&packages).is_empty());
    for dependency in ["api", "model", "provider", "protocol"] {
        let packages = [
            json!({"id":"relay", "name":"relay", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("relay -> {dependency}")]);
    }
}

#[test]
fn provider_and_terminal_sources_do_not_import_metadata_contracts_or_storage() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    for capability in ["provider", "terminal"] {
        let mut pending = vec![crates.join(capability).join("src")];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !source.contains("metadata::"),
                        "{capability} depends on metadata: {}",
                        path.display()
                    );
                }
            }
        }
    }
}
