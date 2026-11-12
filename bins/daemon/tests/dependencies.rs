//! Architectural regression guard, including optional, target-specific, dev and build edges.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// Cargo passes the complete package dependency set to this integration test.
use anyhow as _;
use api as _;
use async_trait as _;
use axum as _;
use base64 as _;
use bot as _;
use browser as _;
use chrono as _;
use clap as _;
use domain as _;
use filesystem as _;
use futures_util as _;
use host_link as _;
use jsonwebtoken as _;
#[cfg(unix)]
use libc as _;
use metadata as _;
use model as _;
use persistence as _;
use provider as _;
use provider_builtin as _;
use reqwest as _;
use ring as _;
#[cfg(unix)]
use rustix as _;
use schedule as _;
use secrecy as _;
use serde as _;
use serde_json::{Value, json};
use tempfile as _;
use terminal as _;
use thiserror as _;
use tokio as _;
use tokio_tungstenite as _;
use tokio_util as _;
use toml as _;
use tracing as _;
use tracing_subscriber as _;
use uuid as _;
use voice as _;

/// Test-only edges that must not reach production: runtime test entry points and adapter-test
/// dependencies.
fn test_only_violations(name: &str, package: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    let forwarded = package["features"]
        .as_object()
        .into_iter()
        .flat_map(|features| features.values())
        .flat_map(|members| members.as_array().into_iter().flatten())
        .filter_map(Value::as_str);
    for member in forwarded.filter(|member| member.ends_with("/test-support")) {
        violations.push(format!("{name} forwards {member}"));
    }
    let features = package["features"].as_object().into_iter().flatten();
    for (feature, members) in features.filter(|(feature, _)| *feature != "test-support") {
        if members
            .as_array()
            .is_some_and(|members| members.iter().any(|member| member == "test-support"))
        {
            violations.push(format!("{name} feature {feature} enables test-support"));
        }
    }
    for dependency in package["dependencies"].as_array().into_iter().flatten() {
        if dependency["kind"].as_str() == Some("dev") {
            continue;
        }
        let target = dependency["name"].as_str().unwrap();
        if dependency["features"]
            .as_array()
            .is_some_and(|features| features.iter().any(|feature| feature == "test-support"))
        {
            violations.push(format!("{name} -> {target} test-support outside tests"));
        }
    }
    violations
}

/// Workspace dependencies `name` may declare only as dev-dependencies, so they stay out of its
/// production build.
fn dev_only_dependencies(name: &str) -> &'static [&'static str] {
    match name {
        "daemon" => &[],
        "provider" => &["persistence", "provider-codex"],
        "api" => &["persistence", "provider-builtin"],
        "provider-builtin" => &["domain", "model", "persistence"],
        "provider-codex" => &["persistence", "provider"],
        "provider-pi"
        | "provider-opencode"
        | "provider-deepseek-harness"
        | "provider-claude"
        | "provider-antigravity"
        | "provider-minimax"
        | "provider-omp" => &["model", "persistence", "provider"],
        _ => &["persistence"],
    }
}

/// Whether `dependency` is optional and enabled only by `package`'s `test-support` feature.
///
/// A feature enables the dependency through `dep:<key>` or the strong `<key>/<feature>` form;
/// the weak `<key>?/<feature>` form does not enable it.
fn enabled_only_by_test_support(package: &Value, dependency: &Value) -> bool {
    let key = dependency["rename"]
        .as_str()
        .or_else(|| dependency["name"].as_str())
        .unwrap();
    let explicit = format!("dep:{key}");
    let strong = format!("{key}/");
    let enables = |member: &Value| {
        member
            .as_str()
            .is_some_and(|member| member == explicit || member.starts_with(&strong))
    };
    dependency["optional"] == true
        && package["features"].as_object().is_some_and(|features| {
            features
                .iter()
                .filter(|(_, members)| {
                    members
                        .as_array()
                        .is_some_and(|members| members.iter().any(enables))
                })
                .map(|(feature, _)| feature.as_str())
                .eq(["test-support"])
        })
}

/// Workspace packages `name` may depend on, or `None` when `name` is not a registered package.
fn allowed_dependencies(name: &str) -> Option<&'static [&'static str]> {
    let allowed: &[&str] = match name {
        "daemon" => &[
            "bot",
            "persistence",
            "host-link",
            "voice",
            "schedule",
            "browser",
            "model",
            "metadata",
            "filesystem",
            "provider",
            "provider-builtin",
            "api",
            "terminal",
            "domain",
        ],
        "api" => &[
            "bot",
            "persistence",
            "host-link",
            "relay",
            "voice",
            "schedule",
            "browser",
            "model",
            "terminal",
            "provider",
            "provider-builtin",
            "domain",
            "metadata",
            "filesystem",
        ],
        "provider-native" => &["domain", "provider-core"],
        "provider-codex"
        | "provider-pi"
        | "provider-opencode"
        | "provider-deepseek-harness"
        | "provider-claude"
        | "provider-antigravity"
        | "provider-minimax"
        | "provider-omp" => &[
            "domain",
            "model",
            "provider-core",
            "provider-native",
            "provider",
            "persistence",
        ],
        "provider" => &[
            "domain",
            "model",
            "persistence",
            "provider-core",
            "provider-codex",
        ],
        "provider-builtin" => &[
            "domain",
            "model",
            "persistence",
            "provider",
            "provider-codex",
            "provider-pi",
            "provider-opencode",
            "provider-deepseek-harness",
            "provider-claude",
            "provider-antigravity",
            "provider-minimax",
            "provider-omp",
        ],
        "persistence" => &["model", "domain"],
        "bot" => &[
            "domain",
            "model",
            "provider",
            "provider-core",
            "persistence",
        ],
        "metadata" | "schedule" | "filesystem" => &["domain", "model", "persistence"],
        "provider-core" | "terminal" => &["domain", "model"],
        "voice" | "browser" | "relay" => &["model"],
        "model" => &["domain"],
        "domain" | "host-link" => &[],
        _ => return None,
    };
    Some(allowed)
}

fn violations(packages: &[Value]) -> Vec<String> {
    let workspace_names: BTreeSet<_> = packages
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    let mut violations = Vec::new();
    for package in packages {
        let name = package["name"].as_str().unwrap();
        violations.extend(test_only_violations(name, package));
        let Some(allowed) = allowed_dependencies(name) else {
            violations.push(format!("unregistered workspace package: {name}"));
            continue;
        };
        let dev_only = dev_only_dependencies(name);
        for dependency in package["dependencies"].as_array().unwrap() {
            let target = dependency["name"].as_str().unwrap();
            if (workspace_names.contains(target) || dependency["path"].is_string())
                && !allowed.contains(&target)
            {
                violations.push(format!("{name} -> {target}"));
            }
            if allowed.contains(&target)
                && dev_only.contains(&target)
                && dependency["kind"].as_str() != Some("dev")
                && !(name == "provider"
                    && target == "persistence"
                    && enabled_only_by_test_support(package, dependency))
            {
                violations.push(format!("{name} -> {target} outside tests"));
            }
            if matches!(
                name,
                "domain" | "persistence" | "model" | "metadata" | "filesystem" | "terminal"
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

/// `cargo metadata` for this workspace; without dependencies, `packages` lists only its members.
fn workspace_metadata() -> Value {
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
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn every_provider_crate_directory_is_a_workspace_member() {
    // ADR-273: the root manifest excludes `crates/provider` from `crates/*` and lists its crates,
    // so Cargo leaves an unlisted one out of the workspace without reporting an error.
    let metadata = workspace_metadata();
    let members: BTreeSet<PathBuf> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| PathBuf::from(package["manifest_path"].as_str().unwrap()))
        .collect();
    let family = Path::new(metadata["workspace_root"].as_str().unwrap()).join("crates/provider");
    let unlisted: Vec<PathBuf> = std::fs::read_dir(family)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("Cargo.toml"))
        .filter(|manifest| manifest.exists() && !members.contains(manifest))
        .collect();
    assert!(
        unlisted.is_empty(),
        "provider crates missing from the workspace members: {unlisted:?}"
    );
}

#[test]
fn all_workspace_dependency_edges_follow_current_adrs() {
    let metadata = workspace_metadata();
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
fn runtime_test_entry_points_stay_out_of_production_builds() {
    let dependency = json!({"name":"provider", "path":"../provider", "features":["test-support"]});
    for (kind, expected) in [
        (
            Value::Null,
            vec![
                "provider-omp -> provider test-support outside tests",
                "provider-omp -> provider outside tests",
            ],
        ),
        (
            json!("build"),
            vec![
                "provider-omp -> provider test-support outside tests",
                "provider-omp -> provider outside tests",
            ],
        ),
        (json!("dev"), vec![]),
    ] {
        let mut dependency = dependency.clone();
        dependency["kind"] = kind;
        let packages = [
            json!({"id":"provider-omp", "name":"provider-omp", "dependencies":[dependency]}),
            json!({"id":"provider", "name":"provider", "dependencies":[]}),
        ];
        assert_eq!(violations(&packages), expected);
    }
}

#[test]
fn runtime_test_entry_points_cannot_be_forwarded_through_features() {
    for feature in ["provider/test-support", "provider?/test-support"] {
        let packages = [
            json!({"id":"bot", "name":"bot", "features":{"default":[feature]}, "dependencies":[]}),
        ];
        assert_eq!(violations(&packages), [format!("bot forwards {feature}")]);
    }
}

#[test]
fn omp_adapter_uses_model_only_in_tests() {
    for (kind, expected) in [
        (Value::Null, vec!["provider-omp -> model outside tests"]),
        (json!("dev"), vec![]),
    ] {
        let packages = [
            json!({"id":"provider-omp", "name":"provider-omp", "dependencies":[{"name":"model", "path":"../model", "kind":kind}]}),
            json!({"id":"model", "name":"model", "dependencies":[]}),
        ];
        assert_eq!(violations(&packages), expected);
    }
}

#[test]
fn provider_runtime_cannot_depend_on_its_omp_adapter_or_the_daemon() {
    for (name, dependency) in [("provider", "provider-omp"), ("provider-omp", "daemon")] {
        let packages = [
            json!({"id":name, "name":name, "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("{name} -> {dependency}")]);
    }
}

#[test]
fn daemon_reaches_adapters_only_through_the_built_in_assembly() {
    let packages = [json!({"id":"daemon", "name":"daemon", "dependencies":[
        {"name":"provider-builtin", "path":"../provider-builtin"},
        {"name":"provider-omp", "path":"../provider-omp"}
    ]})];
    assert_eq!(violations(&packages), ["daemon -> provider-omp"]);
}

#[test]
fn api_uses_the_built_in_assembly_only_in_tests() {
    for (kind, expected) in [
        (Value::Null, vec!["api -> provider-builtin outside tests"]),
        (json!("dev"), vec![]),
    ] {
        let packages = [json!({"id":"api", "name":"api", "dependencies":[
            {"name":"provider-builtin", "path":"../provider-builtin", "kind":kind}
        ]})];
        assert_eq!(violations(&packages), expected);
    }
}

#[test]
fn hosts_and_the_assembly_declare_only_the_family_edges_they_use() {
    let edge = |name: &str, kind: Value| json!({"name": name, "path": "x", "kind": kind});
    for host in ["daemon", "api"] {
        let packages = [json!({"id": host, "name": host, "dependencies": [
            edge("provider", Value::Null),
            edge("provider-core", Value::Null),
        ]})];
        assert_eq!(violations(&packages), [format!("{host} -> provider-core")]);
    }
    let bot = [json!({"id": "bot", "name": "bot", "dependencies": [
        edge("provider", Value::Null),
        edge("provider-core", Value::Null),
    ]})];
    assert!(violations(&bot).is_empty());
    let assembly = |kind: Value| {
        [
            json!({"id": "provider-builtin", "name": "provider-builtin", "dependencies": [
                edge("provider", Value::Null),
                edge("provider-pi", Value::Null),
                edge("provider-core", kind.clone()),
                edge("domain", kind.clone()),
                edge("model", kind.clone()),
                edge("persistence", kind),
            ]}),
        ]
    };
    assert_eq!(
        violations(&assembly(Value::Null)),
        [
            "provider-builtin -> provider-core",
            "provider-builtin -> domain outside tests",
            "provider-builtin -> model outside tests",
            "provider-builtin -> persistence outside tests",
        ]
    );
    assert_eq!(
        violations(&assembly(json!("dev"))),
        ["provider-builtin -> provider-core"]
    );
}

#[test]
fn provider_family_keeps_adapters_and_runtime_apart() {
    let package = |name: &str, dependencies: Value| json!({"id": name, "name": name, "dependencies": dependencies});
    let edge = |name: &str, kind: Value| json!({"name": name, "path": "x", "kind": kind});
    let packages = [
        package("provider-core", json!([edge("provider", Value::Null)])),
        package(
            "provider-native",
            json!([edge("provider-core", Value::Null)]),
        ),
        package(
            "provider",
            json!([
                edge("provider-core", Value::Null),
                edge("provider-native", Value::Null),
                edge("provider-codex", json!("dev")),
                edge("provider-pi", json!("dev")),
            ]),
        ),
        package("provider-codex", json!([edge("provider", json!("dev"))])),
        package("provider-pi", json!([edge("provider", Value::Null)])),
        package(
            "provider-builtin",
            json!([
                edge("provider", Value::Null),
                edge("provider-pi", Value::Null)
            ]),
        ),
    ];
    assert_eq!(
        violations(&packages),
        [
            "provider-core -> provider",
            "provider -> provider-native",
            "provider -> provider-pi",
            "provider-pi -> provider outside tests",
        ]
    );
    let runtime = [
        package("provider", json!([edge("provider-codex", Value::Null)])),
        package("provider-codex", json!([])),
    ];
    assert_eq!(
        violations(&runtime),
        ["provider -> provider-codex outside tests"]
    );
}

#[test]
fn runtime_reaches_persistence_only_through_test_support() {
    let optional = json!({"name": "persistence", "path": "x", "kind": null, "optional": true});
    for (feature, expected) in [
        ("test-support", vec![]),
        ("default", vec!["provider -> persistence outside tests"]),
    ] {
        let packages = [
            json!({"id": "provider", "name": "provider", "dependencies": [optional.clone()],
                "features": {feature: ["dep:persistence"]}}),
            json!({"id": "persistence", "name": "persistence", "dependencies": []}),
        ];
        assert_eq!(violations(&packages), expected);
    }
    for (extra, expected) in [
        (
            "persistence/x",
            vec!["provider -> persistence outside tests"],
        ),
        ("persistence?/x", vec![]),
    ] {
        let packages = [
            json!({"id": "provider", "name": "provider", "dependencies": [optional.clone()],
                "features": {"test-support": ["dep:persistence"], "extra": [extra]}}),
            json!({"id": "persistence", "name": "persistence", "dependencies": []}),
        ];
        assert_eq!(violations(&packages), expected);
    }
}

#[test]
fn test_support_cannot_be_enabled_from_inside_its_own_package() {
    let packages = [
        json!({"id": "provider", "name": "provider", "dependencies": [],
        "features": {"default": ["test-support"], "test-support": []}}),
    ];
    assert_eq!(
        violations(&packages),
        ["provider feature default enables test-support"]
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
        "file",
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
        "persistence",
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
        "persistence",
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
        "persistence",
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
fn persistence_adapters_depend_on_shared_contracts_without_reverse_or_transport_dependencies() {
    for owner in [
        "daemon",
        "metadata",
        "provider",
        "schedule",
        "filesystem",
        "api",
    ] {
        let packages = [
            json!({"name":owner,"dependencies":[{"name":"persistence","path":"../persistence",
                "kind":if owner == "daemon" { Value::Null } else { json!("dev") }
            }]}),
            json!({"name":"persistence","dependencies":[]}),
        ];
        assert!(violations(&packages).is_empty());
    }
    for dependency in ["model", "domain"] {
        let packages = [json!({"name":"persistence","dependencies":[
            {"name":dependency,"path":"../dependency"}
        ]})];
        assert!(violations(&packages).is_empty());
    }
    for dependency in ["metadata", "provider", "schedule", "filesystem", "api"] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"name":"persistence","dependencies":[{
                "name":dependency,"path":"../dependency","rename":"renamed",
                "kind":kind,"optional":true,"target":"cfg(windows)"
            }]})];
            assert_eq!(
                violations(&packages),
                [format!("persistence -> {dependency}")]
            );
        }
    }
    for dependency in ["axum", "rusqlite", "reqwest"] {
        let packages = [json!({"name":"persistence","dependencies":[{"name":dependency}]})];
        assert_eq!(
            violations(&packages),
            [format!("impure persistence -> {dependency}")]
        );
    }
}

#[test]
fn only_the_daemon_injects_persistence_adapters_outside_tests() {
    for owner in [
        "api",
        "bot",
        "metadata",
        "filesystem",
        "provider",
        "schedule",
    ] {
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let packages = [json!({"name":owner,"dependencies":[{
                "name":"persistence","path":"../persistence","rename":"storage",
                "kind":kind,"optional":true,"target":"cfg(windows)"
            }]})];
            let expected = if kind == json!("dev") {
                Vec::new()
            } else {
                vec![format!("{owner} -> persistence outside tests")]
            };
            assert_eq!(violations(&packages), expected);
        }
    }
    for kind in [Value::Null, json!("build")] {
        let packages = [json!({"name":"daemon","dependencies":[{
            "name":"persistence","path":"../persistence","kind":kind
        }]})];
        assert!(violations(&packages).is_empty());
    }
}

#[test]
fn bot_can_use_provider_execution_without_a_reverse_dependency() {
    let allowed = [json!({"name":"bot","dependencies":[
        {"name":"provider","path":"../provider"},
        {"name":"domain","path":"../domain"},
        {"name":"model","path":"../model"}
    ]})];
    assert!(violations(&allowed).is_empty());
    for owner in ["provider", "model", "domain", "persistence", "schedule"] {
        let reversed = [json!({"name":owner,"dependencies":[
            {"name":"bot","path":"../bot"}
        ]})];
        assert_eq!(violations(&reversed), [format!("{owner} -> bot")]);
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
        "persistence",
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
    for dependency in [
        "api",
        "metadata",
        "provider",
        "protocol",
        "domain",
        "persistence",
    ] {
        let packages = [
            json!({"id":"relay", "name":"relay", "dependencies":[{"name":dependency, "path":"../dependency"}]}),
        ];
        assert_eq!(violations(&packages), [format!("relay -> {dependency}")]);
    }
}

#[test]
fn independent_capability_sources_do_not_import_metadata_contracts_or_storage() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    for capability in ["provider/runtime", "terminal", "filesystem"] {
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
