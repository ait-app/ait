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
use file as _;
use filesystem as _;
use futures_util as _;
use metadata as _;
use model as _;
use provider as _;
use reqwest as _;
use schedule as _;
use serde_json::{Value, json};
use tempfile as _;
use terminal as _;
use tokio as _;
use tokio_tungstenite as _;
use tokio_util as _;
use tracing as _;
use tracing_subscriber as _;
use uuid as _;
use voice as _;

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
                "file",
                "voice",
                "schedule",
                "browser",
                "model",
                "metadata",
                "filesystem",
                "provider",
                "api",
                "terminal",
                "domain",
            ],
            "api" => &[
                "file",
                "relay",
                "voice",
                "schedule",
                "browser",
                "model",
                "terminal",
                "provider",
                "domain",
                "metadata",
                "filesystem",
            ],
            "file" => &["model", "domain"],
            "provider" | "metadata" | "schedule" | "filesystem" => &["domain", "model", "file"],
            "terminal" => &["domain", "model"],
            "voice" | "browser" | "relay" => &["model"],
            "model" => &["domain"],
            "domain" => &[],
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
            if target == "file"
                && matches!(name, "api" | "filesystem" | "provider" | "schedule")
                && dependency["kind"].as_str() != Some("dev")
            {
                violations.push(format!("{name} -> file outside tests"));
            }
            if matches!(
                name,
                "domain" | "file" | "model" | "metadata" | "filesystem" | "terminal"
            ) && [
                "sqlx", "rusqlite", "axum", "hyper", "reqwest", "tonic", "tauri", "rig", "codex",
            ]
            .iter()
            .any(|prefix| target == *prefix || target.starts_with(&format!("{prefix}-")))
            {
                violations.push(format!("impure {name} -> {target}"));
            }
            if name == "domain" && (target == "tokio" || target.starts_with("tokio-")) {
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
    for dependency in ["protocol", "provider", "ports", "application", "ait-domain"] {
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
fn filesystem_cannot_depend_on_metadata_host_agent_or_transport_crates() {
    for dependency in [
        "api",
        "protocol",
        "application",
        "ports",
        "workspace",
        "provider",
        "metadata",
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
    for retired in [
        "application",
        "ports",
        "storage",
        "workspace",
        "providers",
        "protocol",
    ] {
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
        "filesystem",
        "metadata",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"terminal", "name":"terminal", "dependencies":[{"name":dependency,"path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("terminal -> {dependency}")]);
    }
    for capability in ["terminal", "filesystem", "provider"] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"id":capability, "name":capability, "dependencies":[{
                "name":"metadata", "rename":"workspace_records", "kind":kind, "optional":true,
                "target":"cfg(windows)", "path":"../metadata"
            }]})];
            assert_eq!(violations(&packages), [format!("{capability} -> metadata")]);
        }
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
fn tokio_is_allowed_in_capability_crates_but_not_domain() {
    for name in [
        "model",
        "file",
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "domain",
    ] {
        for dependency in ["tokio", "tokio-util"] {
            for kind in [Value::Null, json!("dev"), json!("build")] {
                let packages = [
                    json!({"id":name, "name":name, "dependencies":[{"name":dependency,"kind":kind,"optional":true,"target":"cfg(windows)"}]}),
                ];
                let expected = if name == "domain" {
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
        "file",
        "api",
        "protocol",
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "ait-domain",
    ] {
        let packages = [
            json!({"id":"model", "name":"model", "dependencies":[{"name":dependency,"path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("model -> {dependency}")]);
    }
}

#[test]
fn shared_values_are_consumed_directly_from_domain() {
    for owner in [
        "model",
        "file",
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "schedule",
        "api",
        "daemon",
    ] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"name":owner,"dependencies":[{
                "name":"domain", "path":"../domain", "rename":"values",
                "kind":kind, "optional":true, "target":"cfg(windows)"
            }]})];
            assert!(violations(&packages).is_empty(), "{owner} -> domain");
        }
    }
}

#[test]
fn file_adapters_depend_on_shared_contracts_without_reverse_or_transport_dependencies() {
    for owner in [
        "daemon",
        "metadata",
        "provider",
        "schedule",
        "filesystem",
        "api",
    ] {
        let packages = [
            json!({"name":owner,"dependencies":[{"name":"file","path":"../file",
                "kind":if matches!(owner,"daemon"|"metadata") { Value::Null } else { json!("dev") }
            }]}),
            json!({"name":"file","dependencies":[]}),
        ];
        assert!(violations(&packages).is_empty());
    }
    for dependency in ["model", "domain"] {
        let packages =
            [json!({"name":"file","dependencies":[{"name":dependency,"path":"../dependency"}]})];
        assert!(violations(&packages).is_empty());
    }
    for dependency in ["metadata", "provider", "schedule", "filesystem", "api"] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"name":"file","dependencies":[{
                "name":dependency,"path":"../dependency","rename":"renamed",
                "kind":kind,"optional":true,"target":"cfg(windows)"
            }]})];
            assert_eq!(violations(&packages), [format!("file -> {dependency}")]);
        }
    }
    for dependency in ["axum", "rusqlite", "reqwest"] {
        let packages = [json!({"name":"file","dependencies":[{"name":dependency}]})];
        assert_eq!(
            violations(&packages),
            [format!("impure file -> {dependency}")]
        );
    }
}

#[test]
fn consumers_inject_file_adapters_without_production_or_build_dependencies() {
    for owner in ["api", "filesystem", "provider", "schedule"] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"name":owner,"dependencies":[{
                "name":"file","path":"../file","rename":"storage",
                "kind":kind,"optional":true,"target":"cfg(windows)"
            }]})];
            let expected = if kind == json!("dev") {
                Vec::new()
            } else {
                vec![format!("{owner} -> file outside tests")]
            };
            assert_eq!(violations(&packages), expected);
        }
    }
}

#[test]
fn domain_cannot_depend_outward_even_through_test_or_optional_edges() {
    for dependency in [
        "metadata",
        "filesystem",
        "provider",
        "terminal",
        "voice",
        "model",
        "file",
        "api",
        "ait-domain",
    ] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"id":"domain", "name":"domain", "dependencies":[{
                "name":dependency, "path":"../dependency", "rename":"renamed",
                "kind":kind, "optional":true, "target":"cfg(windows)"
            }]})];
            assert_eq!(violations(&packages), [format!("domain -> {dependency}")]);
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
fn relay_owns_rpc_using_model_contracts_without_host_or_business_dependencies() {
    let packages = [
        json!({"id":"api", "name":"api", "dependencies":[{"name":"relay", "path":"../relay"}]}),
        json!({"id":"relay", "name":"relay", "dependencies":[{"name":"model", "path":"../model"}, {"name":"tokio"}, {"name":"reqwest"}]}),
    ];
    assert!(violations(&packages).is_empty());
    for dependency in ["api", "metadata", "provider", "protocol", "domain", "file"] {
        let packages = [
            json!({"id":"relay", "name":"relay", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("relay -> {dependency}")]);
    }
}

#[test]
fn independent_capability_sources_do_not_import_metadata_contracts_or_storage() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    for capability in ["provider", "terminal", "filesystem"] {
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
