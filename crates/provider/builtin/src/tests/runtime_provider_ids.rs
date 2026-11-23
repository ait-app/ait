//! Guard: runtime production code decides by adapter capability, never by comparing against a
//! built-in provider id (ADR-299). The ids come from the registered adapters, so a new adapter is
//! covered without editing this test.

use std::path::{Path, PathBuf};

use provider::testing::clients;

use super::configured;

/// Production literals that may still name a built-in provider, with why each stays.
const ALLOWED: [(&str, &str); 1] = [
    // The legacy agent catalog's schema admits only the Codex driver. Changing the CHECK is a
    // storage migration of persisted rows, not a provider branch in runtime logic.
    ("storage/agents.rs", "'codex'"),
];

/// Module names that hold only test code: `#[cfg(test)]` children and `test-support` entry points.
const TEST_MODULES: [&str; 4] = ["tests", "test_support", "testing", "test_config"];

fn is_test_source(relative: &Path) -> bool {
    relative.with_extension("").components().any(|component| {
        TEST_MODULES
            .iter()
            .any(|name| component.as_os_str() == *name)
    })
}

fn rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
}

/// Every `(file, literal)` in runtime production code that quotes one of `providers`.
fn provider_literals(root: &Path, providers: &[&str]) -> Vec<(String, String)> {
    let mut sources = Vec::new();
    rust_sources(root, &mut sources);
    sources.sort_unstable();
    let mut found = Vec::new();
    for path in sources {
        let relative = path.strip_prefix(root).unwrap();
        if is_test_source(relative) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for provider in providers {
            for literal in [format!("\"{provider}\""), format!("'{provider}'")] {
                if text.contains(&literal) {
                    found.push((relative.to_string_lossy().replace('\\', "/"), literal));
                }
            }
        }
    }
    found
}

#[test]
fn runtime_production_code_names_no_built_in_provider() {
    let data = tempfile::tempdir().unwrap();
    let providers = configured(data.path(), |_| None);
    let ids: Vec<_> = clients(&providers)
        .iter()
        .map(|client| client.provider())
        .collect();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../runtime/src");

    let found = provider_literals(&runtime, &ids);

    let allowed: Vec<_> = ALLOWED
        .iter()
        .map(|(file, literal)| ((*file).to_owned(), (*literal).to_owned()))
        .collect();
    let unexpected: Vec<_> = found.iter().filter(|hit| !allowed.contains(hit)).collect();
    assert!(
        unexpected.is_empty(),
        "runtime branches on provider ids; declare the behavior on the AgentClient port \
         instead: {unexpected:?}"
    );
    let stale: Vec<_> = allowed
        .iter()
        .filter(|entry| !found.contains(entry))
        .collect();
    assert!(
        stale.is_empty(),
        "remove stale allow-list entries: {stale:?}"
    );
}

#[test]
fn guard_flags_quoted_ids_outside_test_modules() {
    let root = tempfile::tempdir().unwrap();
    let write = |relative: &str, text: &str| {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("service/catalog.rs", r#"if id == "native" { label() }"#);
    write("storage/schema.rs", "CHECK(driver='native')");
    write("service/catalog/tests.rs", r#"assert_eq!(id, "native");"#);
    write("service/tests/fixture.rs", r#"const ID: &str = "native";"#);
    write("testing.rs", r#"fn id() -> &'static str { "native" }"#);
    write("service/natively.rs", "// native, \"nativeness\"");

    let found = provider_literals(root.path(), &["native"]);

    assert_eq!(
        found,
        [
            ("service/catalog.rs".to_owned(), "\"native\"".to_owned()),
            ("storage/schema.rs".to_owned(), "'native'".to_owned()),
        ]
    );
}
