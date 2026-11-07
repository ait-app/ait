use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;

/// Managed installation root under a daemon data directory.
fn managed(data_dir: &Path) -> PathBuf {
    data_dir.join("providers/omp-native-host/1")
}

/// Build a manifest tree. `bun` and `entrypoint` are created as regular files.
fn install(root: &Path, manifest: &str, patched: bool) {
    std::fs::create_dir_all(root).unwrap();
    let sdk = root.join("node_modules/@oh-my-pi/pi-coding-agent");
    std::fs::create_dir_all(sdk.join("src/modes/rpc")).unwrap();
    let bun = root.join("node_modules/@oven/bun-linux-x64/bin/bun");
    std::fs::create_dir_all(bun.parent().unwrap()).unwrap();
    std::fs::write(&bun, b"bun").unwrap();
    std::fs::write(sdk.join("src/cli.ts"), b"// sdk").unwrap();
    if patched {
        std::fs::write(
            sdk.join("src/modes/rpc/ait-native-host-hello.ts"),
            b"// hello",
        )
        .unwrap();
    }
    std::fs::write(root.join("manifest.json"), manifest).unwrap();
}

fn manifest(overrides: &str) -> String {
    format!(
        r#"{{"schemaVersion":1,"adapter":"ait-omp-native-host","adapterVersion":1,
"ompVersion":"18.17.0","bunVersion":"1.4.4",
"bunPath":"node_modules/@oven/bun-linux-x64/bin/bun",
"entrypoint":"node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts"{overrides}}}"#
    )
}

fn environment(values: &[(&str, PathBuf)]) -> impl Fn(&str) -> Option<OsString> {
    let values: Vec<(String, OsString)> = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone().into_os_string()))
        .collect();
    move |name| {
        values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn a_verified_manifest_selects_the_bundled_bun_and_patched_entrypoint() {
    let data = tempfile::tempdir().unwrap();
    install(&managed(data.path()), &manifest(""), true);
    let host = discover(data.path(), &environment(&[])).unwrap();
    assert!(
        host.bun()
            .ends_with("node_modules/@oven/bun-linux-x64/bin/bun")
    );
    assert!(
        host.entrypoint()
            .ends_with("node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts")
    );
    assert!(host.entrypoint().is_file());
}

#[test]
fn an_explicit_environment_root_overrides_the_managed_location() {
    let managed = tempfile::tempdir().unwrap();
    let explicit = tempfile::tempdir().unwrap();
    install(explicit.path(), &manifest(""), true);
    let host = discover(
        managed.path(),
        &environment(&[("AIT_OMP_NATIVE_HOST", explicit.path().to_path_buf())]),
    )
    .unwrap();
    assert!(host.bun().starts_with(explicit.path()));
    // The environment variable may also name the manifest itself.
    let host = discover(
        managed.path(),
        &environment(&[("AIT_OMP_NATIVE_HOST", explicit.path().join("manifest.json"))]),
    )
    .unwrap();
    assert!(host.bun().starts_with(explicit.path()));
}

#[test]
fn an_uninstalled_runtime_degrades_to_the_official_binary() {
    let root = tempfile::tempdir().unwrap();
    assert!(discover(root.path(), &environment(&[])).is_none());
    // A named but missing runtime is also absent, never a partial launch.
    assert!(
        discover(
            root.path(),
            &environment(&[("AIT_OMP_NATIVE_HOST", root.path().join("missing"))])
        )
        .is_none()
    );
}

#[test]
fn unpatched_or_mismatched_runtimes_are_refused() {
    let data = tempfile::tempdir().unwrap();
    let root = managed(data.path());
    // Installed but the SDK carries no bridge: launching it would be a shadow runtime.
    install(&root, &manifest(""), false);
    assert!(discover(data.path(), &environment(&[])).is_none());
    // Wrong adapter generation, wrong schema, and a missing SDK version.
    for bad in [
        r#""schemaVersion":1,"adapter":"ait-omp-native-host","adapterVersion":2"#,
        r#""schemaVersion":2,"adapter":"ait-omp-native-host","adapterVersion":1"#,
        r#""schemaVersion":1,"adapter":"other-adapter","adapterVersion":1"#,
    ] {
        let text = format!(
            r#"{{"ompVersion":"18.17.0","bunPath":"node_modules/@oven/bun-linux-x64/bin/bun",
"entrypoint":"node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",{bad}}}"#
        );
        install(&root, &text, true);
        assert!(discover(data.path(), &environment(&[])).is_none(), "{bad}");
    }
    install(&root, &manifest("").replace("\"18.17.0\"", "null"), true);
    assert!(discover(data.path(), &environment(&[])).is_none());
}

#[test]
fn paths_may_not_escape_the_installation_or_be_absolute() {
    let data = tempfile::tempdir().unwrap();
    let root = managed(data.path());
    for bun in [
        "/usr/bin/bun",
        "../bun",
        "node_modules/../../bun",
        "node_modules/@oven/bun-linux-x64/bin/../../../../../../usr/bin/bun",
    ] {
        let text = manifest("").replace(
            "\"bunPath\":\"node_modules/@oven/bun-linux-x64/bin/bun\"",
            &format!("\"bunPath\":\"{bun}\""),
        );
        install(&root, &text, true);
        assert!(discover(data.path(), &environment(&[])).is_none(), "{bun}");
    }
}

#[test]
fn an_oversized_manifest_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let padding = " ".repeat(5000);
    install(
        &managed(data.path()),
        &manifest(&format!(r#","pad":"{padding}""#)),
        true,
    );
    assert!(discover(data.path(), &environment(&[])).is_none());
}

#[test]
fn a_bundle_beside_the_executable_is_used_without_copying_it_into_user_state() {
    let data = tempfile::tempdir().unwrap();
    // No managed installation: discovery must still succeed from a packaged bundle.
    let bundle = data.path().join("provider-runtimes/omp-native-host/1");
    install(&bundle, &manifest(""), true);
    let host = discover(
        data.path(),
        &environment(&[("AIT_SERVER_OMP_NATIVE_HOST_BUNDLE", bundle.clone())]),
    )
    .unwrap();
    assert!(host.bun().starts_with(bundle.canonicalize().unwrap()));
    assert!(!data.path().join("providers/omp-native-host/1").exists());
}

#[test]
fn the_managed_installation_wins_over_a_bundled_copy() {
    let data = tempfile::tempdir().unwrap();
    let bundle = data.path().join("bundle/omp-native-host/1");
    install(&bundle, &manifest(""), true);
    install(&managed(data.path()), &manifest(""), true);
    let host = discover(
        data.path(),
        &environment(&[("AIT_SERVER_OMP_NATIVE_HOST_BUNDLE", bundle)]),
    )
    .unwrap();
    assert!(host.bun().starts_with(data.path().canonicalize().unwrap()));
}

#[test]
fn an_explicit_root_wins_over_both_managed_and_bundled_copies() {
    let data = tempfile::tempdir().unwrap();
    let bundle = data.path().join("bundle/omp-native-host/1");
    install(&bundle, &manifest(""), true);
    install(&managed(data.path()), &manifest(""), true);
    let explicit = tempfile::tempdir().unwrap();
    install(explicit.path(), &manifest(""), true);
    let host = discover(
        data.path(),
        &environment(&[
            ("AIT_SERVER_OMP_NATIVE_HOST_BUNDLE", bundle),
            ("AIT_OMP_NATIVE_HOST", explicit.path().to_path_buf()),
        ]),
    )
    .unwrap();
    assert!(
        host.bun()
            .starts_with(explicit.path().canonicalize().unwrap())
    );
}

#[test]
fn retained_locations_adopt_a_runtime_installed_after_the_first_probe() {
    let data = tempfile::tempdir().unwrap();
    let locations = Locations::new(data.path(), &environment(&[]));
    // Nothing is installed when the daemon composes its clients.
    assert!(locations.discover().is_none());
    // The background installer publishes the managed tree afterwards.
    install(&managed(data.path()), &manifest(""), true);
    let host = locations
        .discover()
        .expect("a published installation is adopted without restarting");
    assert!(
        host.bun()
            .ends_with("node_modules/@oven/bun-linux-x64/bin/bun")
    );
}

#[test]
fn retained_locations_keep_the_environment_overrides_they_were_built_with() {
    let data = tempfile::tempdir().unwrap();
    let explicit = tempfile::tempdir().unwrap();
    let locations = Locations::new(
        data.path(),
        &environment(&[("AIT_OMP_NATIVE_HOST", explicit.path().to_path_buf())]),
    );
    assert!(locations.discover().is_none());
    install(explicit.path(), &manifest(""), true);
    let host = locations.discover().unwrap();
    assert!(
        host.bun()
            .starts_with(explicit.path().canonicalize().unwrap())
    );
}
