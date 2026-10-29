use std::collections::BTreeMap;
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

#[test]
fn cache_configuration_precedes_the_default_home_cache() {
    let mut environment = BTreeMap::from([
        ("HOME", "/home/fixture"),
        ("NPM_CONFIG_CACHE", "/uppercase"),
        ("npm_config_cache", "/lowercase"),
    ]);
    for (expected, removed) in [
        ("/lowercase", "npm_config_cache"),
        ("/uppercase", "NPM_CONFIG_CACHE"),
        ("/home/fixture/.npm", "HOME"),
    ] {
        assert_eq!(
            cache(|key| environment.get(key).map(|value| OsString::from(*value))),
            Some(expected.into())
        );
        environment.remove(removed);
    }
    assert_eq!(cache(|_| None), None);
    assert_eq!(cache(|_| Some("relative-cache".into())), None);
    assert_eq!(
        cache(|key| (key == "HOME").then(|| "relative-home".into())),
        None
    );
    assert_eq!(
        cache(|key| match key {
            "npm_config_cache" => Some(OsString::new()),
            "NPM_CONFIG_CACHE" => Some("/uppercase".into()),
            _ => None,
        }),
        Some("/uppercase".into())
    );
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
