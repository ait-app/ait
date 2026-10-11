//! Capability groups reference each other only through `ports` and `protocol`.

use std::path::{Path, PathBuf};

// Cargo passes the complete package dependency set to this integration test.
use base64 as _;
use chrono as _;
use domain as _;
use getrandom as _;
use metadata as _;
use model as _;
use persistence as _;
use serde as _;
use serde_json as _;
use tempfile as _;
use thiserror as _;
use tokio as _;
use tokio_util as _;
use tracing as _;
use uuid as _;

const GROUPS: [&str; 7] = [
    "automation",
    "daemon",
    "directory",
    "labels",
    "push",
    "server",
    "worktrees",
];
const SHARED_LAYERS: [&str; 2] = ["ports", "protocol"];

/// Return every boundary violation in `source`, owned by the module path `relative`.
fn violations(relative: &Path, source: &str) -> Vec<String> {
    let owner = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map_or("", |name| name.trim_end_matches(".rs"));
    let test_code = relative
        .components()
        .any(|component| component.as_os_str() == "tests")
        || relative.file_name().is_some_and(|name| name == "tests.rs");
    let mut found = Vec::new();
    for (target, layers) in references(source) {
        if owner == "support" && target != "support" {
            found.push(format!("{}: support -> {target}", relative.display()));
        }
        if !GROUPS.contains(&owner) || !GROUPS.contains(&target) || owner == target || test_code {
            continue;
        }
        for layer in layers {
            if !SHARED_LAYERS.contains(&layer.as_str()) {
                found.push(format!(
                    "{}: {owner} -> {target}::{layer}",
                    relative.display()
                ));
            }
        }
    }
    found
}

/// Collect `crate::<module>` references with the layers selected below each one.
fn references(source: &str) -> Vec<(&str, Vec<String>)> {
    let mut found = Vec::new();
    for (index, _) in source.match_indices("crate::") {
        let rest = &source[index + "crate::".len()..];
        let module_end = rest
            .find(|character: char| !(character.is_ascii_lowercase() || character == '_'))
            .unwrap_or(rest.len());
        let module = &rest[..module_end];
        let after = &rest[module_end..];
        let layers = if let Some(selected) = after.strip_prefix("::{") {
            let end = selected.find('}').unwrap_or(selected.len());
            selected[..end]
                .split(',')
                .filter_map(|item| item.trim().split("::").next())
                .filter(|layer| !layer.is_empty())
                .map(str::to_owned)
                .collect()
        } else if let Some(path) = after.strip_prefix("::") {
            let end = path
                .find(|character: char| !(character.is_ascii_lowercase() || character == '_'))
                .unwrap_or(path.len());
            vec![path[..end].to_owned()]
        } else {
            Vec::new()
        };
        found.push((module, layers));
    }
    found
}

fn sources(directory: &Path, pending: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("readable source directory") {
        let path = entry.expect("readable source entry").path();
        if path.is_dir() {
            sources(&path, pending);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            pending.push(path);
        }
    }
}

#[test]
fn production_groups_use_only_shared_ports_and_protocols_of_other_groups() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let found: Vec<_> = files
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).expect("readable source");
            violations(path.strip_prefix(&root).expect("source below src"), &source)
        })
        .collect();
    assert!(found.is_empty(), "module boundary violations: {found:#?}");
    for group in GROUPS {
        assert!(
            root.join(group).is_dir(),
            "missing capability group {group}"
        );
    }
}

#[test]
fn guard_rejects_concrete_cross_group_layers_and_support_dependencies() {
    let creation = Path::new("server/connection/creation.rs");
    assert!(
        violations(
            creation,
            "use crate::directory::protocol::directory::X;\nuse crate::worktrees::ports::worktrees::Y;"
        )
        .is_empty()
    );
    assert_eq!(
        violations(
            creation,
            "crate::directory::rpc::directory::workspace_creation"
        ),
        ["server/connection/creation.rs: server -> directory::rpc"]
    );
    assert_eq!(
        violations(
            Path::new("labels/rpc/workspace_labels.rs"),
            "use crate::directory::{protocol::directory::A, service::directory::B};"
        ),
        ["labels/rpc/workspace_labels.rs: labels -> directory::service"]
    );
    for layer in ["service", "rpc", "connection", "local"] {
        let source = format!("crate::automation::{layer}::workspace_automation::X");
        assert_eq!(
            violations(Path::new("worktrees/service/worktrees.rs"), &source),
            [format!(
                "worktrees/service/worktrees.rs: worktrees -> automation::{layer}"
            )]
        );
    }
    assert!(
        violations(
            Path::new("directory/service/directory/tests.rs"),
            "crate::worktrees::service::worktrees::Worktrees"
        )
        .is_empty()
    );
    for module in ["dispatch", "installation", "collaboration", "connection"] {
        assert!(
            violations(
                Path::new(&format!("{module}.rs")),
                "crate::automation::service::workspace_automation::WorkspaceAutomation"
            )
            .is_empty()
        );
    }
    assert_eq!(
        violations(
            Path::new("support/error.rs"),
            "crate::labels::service::workspace_labels::X; crate::dispatch::State; crate::support::error::E"
        ),
        [
            "support/error.rs: support -> labels",
            "support/error.rs: support -> dispatch"
        ]
    );
}
