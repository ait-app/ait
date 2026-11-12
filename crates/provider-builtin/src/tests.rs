use std::ffi::OsString;

use domain::agent_runtime::StoredAgentConfig;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use provider::service::agent_manager::{AgentManager, AgentManagerError};
use provider::testing::{Manager, clients, summary_clients};

use super::*;

#[cfg(unix)]
mod summary;

/// Every built-in provider identity, sorted; registration order is not observable.
const BUILT_IN: [&str; 8] = [
    "antigravity",
    "claude",
    "codex",
    "deepseek-harness",
    "minimax-code",
    "omp",
    "opencode",
    "pi",
];

#[test]
fn the_built_in_set_registers_every_adapter_once() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("absent");
    let mut manager = AgentManager::new(Box::new(FileBackedAgentRuntimeRegistry::new(
        data.join("agents.json"),
    )));

    let providers = configured(&data, |_| None);
    let mut registered: Vec<_> = clients(&providers)
        .iter()
        .map(|client| client.provider())
        .collect();
    registered.sort_unstable();
    assert_eq!(registered, BUILT_IN);
    providers.register(&mut manager).unwrap();
    assert!(matches!(
        configured(&data, |_| None).register(&mut manager),
        Err(AgentManagerError::AlreadyExists(_))
    ));
    let manager = Manager::new(manager);
    for provider in BUILT_IN {
        manager.validate_provider(provider).unwrap();
    }
    assert!(!data.exists());
}

#[tokio::test]
async fn explicit_missing_executables_do_not_fall_back_to_installed_programs() {
    let root = tempfile::tempdir().unwrap();
    let providers = configured(root.path(), |name| {
        name.ends_with("_BIN")
            .then(|| root.path().join(name).into_os_string())
    });

    for client in clients(&providers) {
        assert!(
            !client.is_available().await.unwrap(),
            "{}",
            client.provider()
        );
    }
    for client in summary_clients(&providers) {
        assert!(
            !client.is_available().await.unwrap(),
            "{}",
            client.provider()
        );
    }
}

#[test]
fn dsh_transport_override_preserves_native_and_acp_capabilities() {
    let root = tempfile::tempdir().unwrap();
    for (transport, native) in [(None, true), (Some("acp"), false), (Some("ACP"), true)] {
        let providers = configured(root.path(), |name| {
            (name == "AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT")
                .then(|| transport.map(OsString::from))
                .flatten()
        });
        let dsh = clients(&providers)
            .iter()
            .find(|client| client.provider() == "deepseek-harness")
            .unwrap();
        assert_eq!(dsh.supports_history_replay(), native);
        assert_eq!(
            dsh.settings(&StoredAgentConfig::default())["capabilities"]["supportsDynamicModes"],
            native
        );
    }
}
