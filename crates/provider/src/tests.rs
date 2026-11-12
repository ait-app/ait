//! Temporary guard for the provider crate split (ADR-270): each layer may reference only the
//! modules its future crate will depend on. Deleted once the layers are separate crates.

use std::fs;
use std::path::Path;

/// Modules that stay in the runtime crate.
const RUNTIME: &[&str] = &[
    "service",
    "rpc",
    "storage",
    "dispatch",
    "connection",
    "support",
    "timeline_projection",
    "skills",
    "capabilities",
    "composition",
    "installation",
    "diagnostics",
    "testing",
];

/// `local` modules that move to `provider-native`.
const TOOLKIT: &[&str] = &[
    "acp_config",
    "acp_permissions",
    "acp_streaming",
    "acp_transport",
    "compaction",
    "configuration",
    "diagnostics",
    "elicitation",
    "images",
    "metadata_process",
    "notes",
    "process_group",
    "prompt_blocks",
    "sandbox",
    "session_preview",
    "summary_model",
    "tool_detail",
    "usage",
];

/// `local` modules that become one crate each.
const ADAPTERS: &[&str] = &[
    "antigravity",
    "claude",
    "codex",
    "deepseek_harness",
    "minimax",
    "opencode",
    "pi",
];

/// Runtime tests that build a non-Codex adapter until they move to `provider-builtin`.
const PENDING: &[&str] = &[
    "service/provider_catalog/tests.rs",
    "service/provider_catalog/tests/diagnostics.rs",
    "service/agent_execution/tests/subscriptions/creation.rs",
    "composition/tests.rs",
];

#[test]
fn layers_reference_only_what_their_future_crate_may_depend_on() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();

    let mut crossings = Vec::new();
    for file in &files {
        let source = fs::read_to_string(root.join(file)).unwrap();
        for (line, target) in references(&source) {
            let entry = format!("{file}:{line}");
            if crosses(file, &target) && crossings.last() != Some(&entry) {
                crossings.push(entry);
            }
        }
    }

    assert!(
        crossings.is_empty(),
        "cross-layer references:\n{}",
        crossings.join("\n")
    );
}

#[test]
fn references_read_indented_and_visibility_prefixed_groups() {
    let source =
        "fn f() {\n    use crate::service::x::{A, B};\n}\npub(crate) use crate::ports::y::{C};\n";
    assert_eq!(
        references(source),
        [
            (2, "service::x::A".to_owned()),
            (2, "service::x::B".to_owned()),
            (4, "ports::y::C".to_owned()),
        ]
    );
}

/// Every `.rs` file under `dir`, relative to `root` with `/` separators.
fn collect(dir: &Path, root: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, root, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path.strip_prefix(root).unwrap().to_string_lossy();
            files.push(relative.replace('\\', "/"));
        }
    }
}

/// `(line, path after crate::)` for every `crate::` path and every entry of a `use crate::` group.
fn references(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    // Prefix stack of the open `use crate::...{...}` group, if any.
    let mut group: Vec<String> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let number = index + 1;
        for (start, _) in line.match_indices("crate::") {
            let rest = &line[start + "crate::".len()..];
            let length = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(rest.len());
            // A path followed by `{` opens a group, whose entries are read below.
            let path = rest[..length].trim_end_matches(':');
            if !path.is_empty() && !rest[length..].starts_with('{') {
                found.push((number, path.to_owned()));
            }
        }
        let opened = group
            .is_empty()
            .then(|| {
                use_statement(line)?
                    .strip_prefix("crate::")?
                    .split_once('{')
            })
            .flatten();
        let entries = match opened {
            Some((prefix, rest)) => {
                group.push(prefix.to_owned());
                rest
            }
            None if !group.is_empty() => line,
            None => continue,
        };
        let mut segment = String::new();
        for c in entries.chars() {
            match c {
                '{' => {
                    let prefix = format!("{}{segment}", group.last().unwrap());
                    group.push(prefix);
                    segment.clear();
                }
                ',' | '}' => {
                    if !segment.is_empty() {
                        found.push((number, format!("{}{segment}", group.last().unwrap())));
                        segment.clear();
                    }
                    if c == '}' {
                        group.pop();
                        if group.is_empty() {
                            break;
                        }
                    }
                }
                c if !c.is_whitespace() => segment.push(c),
                _ => {}
            }
        }
        if let Some(prefix) = group.last().filter(|_| !segment.is_empty()) {
            found.push((number, format!("{prefix}{segment}")));
        }
    }
    found
}

/// The rest of `line` after an indented, optionally `pub` or `pub(...)`, `use `.
fn use_statement(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let line = match line.strip_prefix("pub") {
        Some(rest) => rest
            .strip_prefix('(')
            .map_or(rest, |scope| {
                scope.split_once(')').map_or(scope, |(_, after)| after)
            })
            .trim_start(),
        None => line,
    };
    line.strip_prefix("use ")
}

/// Whether `file` referencing `crate::<target>` crosses a future crate boundary.
fn crosses(file: &str, target: &str) -> bool {
    let mut segments = target.split("::");
    let top = segments.next().unwrap_or_default();
    let module = segments.next().unwrap_or_default();
    let adapter = top == "local" && ADAPTERS.contains(&module);
    let test = file == "local/agent_mcp_injection.rs"
        || file.split('/').any(|part| {
            matches!(part, "tests" | "fixtures" | "tests.rs") || part.starts_with("test_support")
        });

    if let Some(local) = file.strip_prefix("local/") {
        let own = local.split(['/', '.']).next().unwrap_or_default();
        (!test && RUNTIME.contains(&top)) || (TOOLKIT.contains(&own) && adapter)
    } else if test {
        adapter && module != "codex" && !PENDING.contains(&file)
    } else {
        top == "local" && file != "composition.rs"
    }
}
