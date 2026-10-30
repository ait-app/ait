use std::fs::{File, FileTimes};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::super::{
    resolve,
    tests::{bundle, file},
};
use super::*;
use crate::ports::agent_session::AgentClient;

const MANIFEST: &str = r#"{"name":"@deepseek-ai/dsh","bin":{"dsh":"lib/bin.js"}}"#;

fn package(cache: &Path, name: &str, modified: u64) -> PathBuf {
    let root = cache
        .join("_npx")
        .join(name)
        .join("node_modules/@deepseek-ai/dsh");
    file(&root.join("package.json"), MANIFEST, false);
    let program = root.join("lib/bin.js");
    file(
        &program,
        "#!/bin/sh\nprintf 'cached-cli-fixture\\n'\n",
        true,
    );
    File::open(root)
        .unwrap()
        .set_times(
            FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(modified)),
        )
        .unwrap();
    program
}

#[tokio::test]
async fn cached_installation_runs_without_a_path_cli() {
    let root = tempfile::tempdir().unwrap();
    let program = package(root.path(), "cached", 1);

    let client = resolve(&[], &[], Some(root.path()));

    assert_eq!(client.program, program);
    assert!(client.is_available().await.unwrap());
    let output = client.command().arg("--version").output().await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"cached-cli-fixture\n");
}

#[test]
fn path_and_desktop_installations_precede_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let cached = package(root.path(), "cached", 1);
    let (desktop_program, _) = bundle(root.path(), false);
    let bin = root.path().join("bin");
    let cli = bin.join("dsh");
    file(&cli, "#!/bin/sh\nexit 0\n", true);
    let paths = [bin];
    let bundles = [root.path().into()];

    assert_eq!(resolve(&paths, &bundles, Some(root.path())).program, cli);
    std::fs::remove_file(cli).unwrap();
    assert_eq!(
        resolve(&paths, &bundles, Some(root.path())).program,
        desktop_program
    );
    std::fs::remove_file(desktop_program).unwrap();
    assert_eq!(resolve(&paths, &bundles, Some(root.path())).program, cached);
}

#[test]
fn invalid_or_incomplete_cached_packages_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(cached_cli(root.path()), None);
    let program = package(root.path(), "cached", 1);
    let manifest = program
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("package.json");
    for contents in [
        "not json",
        r#"{"name":"unrelated","bin":{"dsh":"lib/bin.js"}}"#,
        r#"{"name":"@deepseek-ai/dsh","bin":{"dsh":"other.js"}}"#,
        &" ".repeat(64 * 1024 + 1),
    ] {
        file(&manifest, contents, false);
        assert_eq!(cached_cli(root.path()), None);
    }
    file(&manifest, MANIFEST, false);
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(cached_cli(root.path()), None);
    std::fs::remove_file(program).unwrap();
    assert_eq!(cached_cli(root.path()), None);
    std::fs::remove_file(manifest).unwrap();
    assert_eq!(cached_cli(root.path()), None);
}

#[test]
fn newest_complete_installation_is_selected_with_a_deterministic_tie_break() {
    let root = tempfile::tempdir().unwrap();
    let older = package(root.path(), "older", 1);
    let newer = package(root.path(), "newer", 2);
    assert_eq!(cached_cli(root.path()), Some(newer.clone()));
    let tied = package(root.path(), "newer-tie", 2);
    assert_eq!(
        cached_cli(root.path()),
        Some(std::cmp::max(newer.clone(), tied.clone()))
    );
    std::fs::remove_file(newer).unwrap();
    std::fs::remove_file(tied).unwrap();
    assert_eq!(cached_cli(root.path()), Some(older));
}

#[test]
fn cached_entrypoint_comes_from_the_package_manifest() {
    let root = tempfile::tempdir().unwrap();
    let original = package(root.path(), "cached", 1);
    let package = original.parent().unwrap().parent().unwrap();
    let declared = package.join("dist/custom-cli.cjs");
    file(&declared, "#!/bin/sh\nexit 0\n", true);
    for bin in [
        serde_json::json!({"dsh":"dist/custom-cli.cjs"}),
        serde_json::json!("dist/custom-cli.cjs"),
    ] {
        file(
            &package.join("package.json"),
            &serde_json::json!({"name":"@deepseek-ai/dsh","bin":bin}).to_string(),
            false,
        );
        assert_eq!(cached_cli(root.path()), Some(declared.clone()));
    }
    for entrypoint in ["../outside", "/outside", "", "."] {
        file(
            &package.join("package.json"),
            &serde_json::json!({"name":"@deepseek-ai/dsh","bin":{"dsh":entrypoint}}).to_string(),
            false,
        );
        assert_eq!(cached_cli(root.path()), None);
    }
}
