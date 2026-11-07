//! Adopting a runtime that the daemon installed after the client was composed.

use super::*;
use std::ffi::OsString;

/// Build a verifiable managed installation under `data_dir`.
fn install(data_dir: &std::path::Path) {
    let root = data_dir.join("providers/omp-native-host/1");
    let bun = root.join("node_modules/@oven/bun-linux-x64/bin/bun");
    let sdk = root.join("node_modules/@oh-my-pi/pi-coding-agent");
    std::fs::create_dir_all(bun.parent().unwrap()).unwrap();
    std::fs::create_dir_all(sdk.join("src/modes/rpc")).unwrap();
    std::fs::write(&bun, b"bun").unwrap();
    std::fs::write(sdk.join("src/cli.ts"), b"// sdk").unwrap();
    std::fs::write(
        sdk.join("src/modes/rpc/ait-native-host-hello.ts"),
        b"// hello",
    )
    .unwrap();
    std::fs::write(
        root.join("manifest.json"),
        r#"{"schemaVersion":1,"adapter":"ait-omp-native-host","adapterVersion":1,
"ompVersion":"18.17.0","bunVersion":"1.4.4",
"bunPath":"node_modules/@oven/bun-linux-x64/bin/bun",
"entrypoint":"node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts"}"#,
    )
    .unwrap();
}

fn environment() -> impl Fn(&str) -> Option<OsString> {
    |_| None
}

/// Compose the client the way the daemon does, before any installation exists.
fn client(data_dir: &std::path::Path) -> OmpClient {
    let locations = native_host::Locations::new(data_dir, &environment());
    let host = locations.discover();
    OmpClient::new("missing".into()).watching_native_host(locations, host)
}

#[test]
fn a_client_composed_before_the_install_adopts_the_runtime_without_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let client = client(data.path());
    assert!(client.native_host().is_none());
    // The daemon's background installer finishes after composition.
    install(data.path());
    client.refresh_native_host();
    let host = client
        .native_host()
        .expect("the next session adopts the installed runtime");
    assert!(
        host.bun()
            .ends_with("node_modules/@oven/bun-linux-x64/bin/bun")
    );
}

#[test]
fn a_client_that_already_verified_the_runtime_never_probes_again() {
    let data = tempfile::tempdir().unwrap();
    install(data.path());
    let client = client(data.path());
    let adopted = client.native_host().expect("composed after the install");
    // Removing the tree must not move a live client back to the official binary mid-daemon.
    std::fs::remove_dir_all(data.path().join("providers")).unwrap();
    client.refresh_native_host();
    let kept = client.native_host().unwrap();
    assert_eq!(kept.bun(), adopted.bun());
}

#[test]
fn clones_share_the_adopted_runtime() {
    let data = tempfile::tempdir().unwrap();
    let client = client(data.path());
    let clone = client.clone();
    install(data.path());
    clone.refresh_native_host();
    assert!(
        client.native_host().is_some(),
        "a refresh through one clone is visible to the manager's copy"
    );
}

#[test]
fn a_client_without_candidates_stays_on_the_official_binary() {
    let client = OmpClient::new("missing".into());
    client.refresh_native_host();
    assert!(client.native_host().is_none());
}

#[test]
fn repeated_refreshes_after_a_miss_do_not_reread_the_disk() {
    let data = tempfile::tempdir().unwrap();
    let client = client(data.path());
    client.refresh_native_host();
    // A burst of new sessions within the probe interval must not stat the manifests again, so a
    // runtime published after the first miss is only picked up once the interval elapses.
    install(data.path());
    client.refresh_native_host();
    assert!(client.native_host().is_none());
}
