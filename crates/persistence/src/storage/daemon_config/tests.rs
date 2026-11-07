use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

use model::storage::daemon_config::DaemonConfigStore;
use serde_json::json;

use super::*;

#[test]
fn new_daemon_enables_injection_and_preserves_explicit_opt_out() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let store = FileDaemonConfigStore::with_defaults(path.clone());
    assert_eq!(store.get().unwrap()["mcp"]["injectIntoAgents"], true);
    store
        .patch(&json!({"mcp":{"injectIntoAgents":false}}))
        .unwrap();
    let reopened = FileDaemonConfigStore::with_defaults(path);
    assert_eq!(reopened.get().unwrap()["mcp"]["injectIntoAgents"], false);
}

#[test]
fn invalid_known_fields_do_not_change_the_persisted_config() {
    let root = tempfile::tempdir().unwrap();
    let store = store(&root);
    let before = store.patch(&json!({})).unwrap();
    let original = fs::read(root.path().join("config.json")).unwrap();
    for patch in [
        json!({"mcp":{"injectIntoAgents":1}}),
        json!({"browserTools":{"enabled":"yes"}}),
        json!({"providers":{"":{}}}),
        json!({"providers":{"codex":false}}),
        json!({"metadataGeneration":{"providers":[{"provider":""}]}}),
        json!({"appendSystemPrompt":false}),
        json!({"relay":{"enabled":1}}),
    ] {
        assert!(
            matches!(store.patch(&patch), Err(DaemonConfigStoreError::Invalid)),
            "{patch}"
        );
        assert_eq!(store.get().unwrap(), before);
        assert_eq!(fs::read(root.path().join("config.json")).unwrap(), original);
    }
    let mut no_optional_mcp = before.clone();
    no_optional_mcp["mcp"]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    validate_config(&no_optional_mcp).unwrap();
    for (key, value) in [
        (
            "git",
            json!({"maxProcessesPerSecond":0,"maxProcessConcurrency":1}),
        ),
        ("catalogRefreshTimeoutMs", json!(0)),
    ] {
        let mut invalid = before.clone();
        invalid[key] = value;
        fs::write(root.path().join("config.json"), invalid.to_string()).unwrap();
        assert!(matches!(
            store.reload(),
            Err(DaemonConfigStoreError::Invalid)
        ));
        assert_eq!(store.get().unwrap(), before);
    }
    fs::write(root.path().join("config.json"), "[]").unwrap();
    assert!(matches!(
        store.reload(),
        Err(DaemonConfigStoreError::Invalid)
    ));
    assert_eq!(store.get().unwrap(), before);
}

fn store(directory: &tempfile::TempDir) -> FileDaemonConfigStore {
    FileDaemonConfigStore::new(
        directory.path().join("config.json"),
        json!({
            "relay":{"enabled":false},
            "mcp":{"enabled":true,"injectIntoAgents":false},
            "browserTools":{"enabled":false},
            "providers":{"codex":{"enabled":true}},
            "metadataGeneration":{"providers":[{"provider":"codex"}]},
            "autoArchiveAfterMerge":false,
            "enableTerminalAgentHooks":false,
            "appendSystemPrompt":""
        }),
    )
}

#[test]
fn initializes_and_atomically_patches_supported_fields() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory);
    assert!(!directory.path().join("config.json").exists());
    assert_eq!(store.get().unwrap()["relay"]["enabled"], false);
    let next = store
        .patch(&json!({
            "relay":{"enabled":true},
            "providers":{"codex":{"enabled":false,"future":1}},
            "unknown":{"ignored":true}
        }))
        .unwrap();
    assert_eq!(next["relay"]["enabled"], true);
    assert_eq!(next["providers"]["codex"]["enabled"], false);
    assert_eq!(next["providers"]["codex"]["future"], 1);
    assert!(next.get("unknown").is_none());
    let persisted: Value =
        serde_json::from_slice(&fs::read(directory.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(persisted, next);
}

#[test]
fn provider_removal_updates_overrides_and_metadata_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory);
    let next = store.patch(&json!({"removeProviders":["codex"]})).unwrap();
    assert!(next["providers"].as_object().unwrap().is_empty());
    assert!(
        next["metadataGeneration"]["providers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reload_classifies_live_and_restart_paths() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory);
    store.get().unwrap();
    fs::write(
        directory.path().join("config.json"),
        serde_json::to_vec_pretty(&json!({
            "relay":{"enabled":true},
            "mcp":{"enabled":true,"injectIntoAgents":false},
            "browserTools":{"enabled":false},
            "providers":{"codex":{"enabled":true}},
            "metadataGeneration":{"providers":[{"provider":"codex"}]},
            "autoArchiveAfterMerge":false,
            "enableTerminalAgentHooks":false,
            "appendSystemPrompt":"",
            "plugins":{"local":{"source":"directory"}},
            "futureRoot":true
        }))
        .unwrap(),
    )
    .unwrap();
    let reloaded = store.reload().unwrap();
    assert_eq!(reloaded.applied_paths, ["daemon.relay.enabled"]);
    assert_eq!(reloaded.restart_required_paths, ["futureRoot", "plugins"]);
    assert!(reloaded.override_controlled_paths.is_empty());
}

#[test]
fn invalid_or_symlink_documents_are_rejected_without_replacing_current() {
    let directory = tempfile::tempdir().unwrap();
    let config_store = store(&directory);
    let current = config_store.get().unwrap();
    fs::write(directory.path().join("config.json"), b"not-json").unwrap();
    assert_eq!(config_store.reload(), Err(DaemonConfigStoreError::Invalid));
    assert_eq!(config_store.get().unwrap(), current);

    #[cfg(unix)]
    {
        let other = directory.path().join("other.json");
        fs::write(&other, b"{}").unwrap();
        fs::remove_file(directory.path().join("config.json")).unwrap();
        std::os::unix::fs::symlink(other, directory.path().join("config.json")).unwrap();
        let fresh = store(&directory);
        assert_eq!(fresh.get(), Err(DaemonConfigStoreError::Io));
    }
}

#[test]
fn concurrent_reload_never_installs_a_config_older_than_a_completed_patch() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory);
    store.get().unwrap();
    let patching = AtomicBool::new(true);
    let stale_reads = std::thread::scope(|scope| {
        scope.spawn(|| {
            while patching.load(Ordering::Relaxed) {
                store.reload().unwrap();
            }
        });
        let stale_reads = (0..200)
            .filter(|revision| {
                let prompt = format!("revision-{revision}");
                store.patch(&json!({"appendSystemPrompt": prompt})).unwrap();
                store.get().unwrap()["appendSystemPrompt"] != prompt
            })
            .count();
        patching.store(false, Ordering::Relaxed);
        stale_reads
    });
    assert_eq!(stale_reads, 0);
}

#[test]
fn provider_model_presets_round_trip_without_touching_another_provider() {
    let root = tempfile::tempdir().unwrap();
    let store = store(&root);
    store
        .patch(&json!({"providers":{"claude":{"modelPresets":[
            {"name":"team","model":"opus","thinkingOptionId":"high",
             "featureValues":{"modelRole.advisor":"sonnet"}}]}}}))
        .unwrap();
    store
        .patch(&json!({"providers":{"codex":{"modelPresets":[
            {"name":"fast","model":"gpt-5"}]}}}))
        .unwrap();
    let reopened = FileDaemonConfigStore::new(
        root.path().join("config.json"),
        json!({"relay":{"enabled":false},"mcp":{"enabled":true,"injectIntoAgents":false},
            "browserTools":{"enabled":false},"providers":{},
            "metadataGeneration":{"providers":[]}}),
    );
    let providers = reopened.get().unwrap()["providers"].clone();
    assert_eq!(
        providers["claude"]["modelPresets"][0]["featureValues"]["modelRole.advisor"],
        "sonnet"
    );
    assert_eq!(providers["claude"]["modelPresets"][0]["thinkingOptionId"], "high");
    assert_eq!(providers["codex"]["modelPresets"][0]["model"], "gpt-5");
    assert_eq!(providers["codex"]["enabled"], true);
}

#[test]
fn provider_model_presets_reject_malformed_entries() {
    let root = tempfile::tempdir().unwrap();
    let store = store(&root);
    for preset in [
        json!({"name":"","model":"opus"}),
        json!({"name":"team"}),
        json!({"name":"team","model":""}),
        json!({"name":"team","model":"opus","featureValues":{"1bad":"x"}}),
        json!({"name":"team","model":"opus","featureValues":{"modelRole.advisor":null}}),
        json!({"name":"team","model":"opus","thinkingOptionId":""}),
    ] {
        assert!(
            matches!(
                store.patch(&json!({"providers":{"claude":{"modelPresets":[preset]}}})),
                Err(DaemonConfigStoreError::Invalid)
            ),
            "{preset}"
        );
    }
}
