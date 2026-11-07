use super::*;

/// Resolve one environment value from a fixed table, mirroring the daemon's lookup.
fn value(values: &[(&str, PathBuf)], name: &str) -> Option<std::ffi::OsString> {
    values
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.clone().into_os_string())
}

#[test]
fn a_missing_runtime_is_a_quiet_no_op() {
    // No managed tree, no bundle and no installer: startup must stay a successful no-op.
    let directory = tempfile::tempdir().unwrap();
    workspace(directory.path(), &|_| None);
    assert!(
        !directory
            .path()
            .join("providers/omp-native-host/1")
            .exists()
    );
}

#[test]
fn a_failing_installer_keeps_the_official_binary_instead_of_failing() {
    let directory = tempfile::tempdir().unwrap();
    let installer = directory.path().join("install.mjs");
    std::fs::write(&installer, "process.exit(3);\n").unwrap();
    let values = [("AIT_SERVER_OMP_NATIVE_HOST_INSTALLER", installer)];
    // A non-existent Node binary stands in for a broken toolchain; the probe must not panic.
    workspace(directory.path(), &|name| match name {
        "AIT_SERVER_OMP_NATIVE_HOST_INSTALLER" => value(&values, name),
        "AIT_SERVER_NODE_BIN" => Some("/nonexistent/node".into()),
        _ => None,
    });
    assert!(
        !directory
            .path()
            .join("providers/omp-native-host/1")
            .exists()
    );
}

#[test]
fn an_invalid_installed_manifest_is_not_reinstalled_or_trusted() {
    let directory = tempfile::tempdir().unwrap();
    let managed = directory.path().join("providers/omp-native-host/1");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::write(managed.join("manifest.json"), "{\"schemaVersion\":1}").unwrap();
    workspace(directory.path(), &|_| None);
    // The malformed manifest stays exactly as found: no partial tree appears next to it.
    assert_eq!(
        std::fs::read(managed.join("manifest.json")).unwrap(),
        b"{\"schemaVersion\":1}"
    );
    assert!(!managed.join("node_modules").exists());
}

#[cfg(unix)]
mod supervised;
