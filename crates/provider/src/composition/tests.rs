use super::*;

#[cfg(unix)]
use metadata::ports::generation::{MetadataKind, MetadataRequest, MetadataSelection};
#[cfg(unix)]
use metadata::storage::daemon_config::FileDaemonConfigStore;
#[cfg(unix)]
use serde_json::json;

use crate::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

#[test]
fn registration_preserves_the_catalog_without_startup_writes_and_rejects_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("absent");
    let mut manager = AgentManager::new(Box::new(FileBackedAgentRuntimeRegistry::new(
        data.join("agents.json"),
    )));

    Providers::new(&data).register(&mut manager).unwrap();
    let (_, clients) = manager.take_catalog();
    assert_eq!(
        clients.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "antigravity",
            "claude",
            "codex",
            "deepseek-harness",
            "opencode"
        ]
    );
    assert!(matches!(
        Providers::new(&data).register(&mut manager),
        Err(AgentManagerError::AlreadyExists(provider)) if provider == "codex"
    ));
    assert!(!data.exists());
}

#[tokio::test]
async fn explicit_missing_executables_do_not_fall_back_to_installed_programs() {
    let root = tempfile::tempdir().unwrap();
    let providers = Providers::configured(root.path(), |name| {
        name.ends_with("_BIN")
            .then(|| root.path().join(name).into_os_string())
    });

    for client in &providers.clients {
        assert!(
            !client.is_available().await.unwrap(),
            "{}",
            client.provider()
        );
    }
    for client in &providers.metadata_clients {
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
        let providers = Providers::configured(root.path(), |name| {
            (name == "AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT")
                .then(|| transport.map(OsString::from))
                .flatten()
        });
        let dsh = providers
            .clients
            .iter()
            .find(|client| client.provider() == "deepseek-harness")
            .unwrap();
        assert_eq!(dsh.supports_history_replay(), native);
        assert_eq!(
            dsh.settings(&domain::agent_runtime::StoredAgentConfig::default())["capabilities"]["supportsDynamicModes"],
            native
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn configured_codex_and_claude_generate_metadata_without_foreground_agents() {
    let fixture = crate::test_support::Fixture::new();
    let config = Arc::new(FileDaemonConfigStore::with_defaults(
        fixture.root.path().join("config.json"),
    ));
    let providers = Providers::configured(fixture.root.path(), |name| {
        Some(match name {
            "AIT_SERVER_CODEX_BIN" => fixture.program.clone().into_os_string(),
            "AIT_SERVER_CLAUDE_BIN" => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude_code.py")
                .into_os_string(),
            _ => fixture.root.path().join(name).into_os_string(),
        })
    });
    let generator = providers.metadata_generator(config.clone());
    std::fs::write(
        fixture.cwd.join("metadata-response.json"),
        r#"{"title":"Configured metadata"}"#,
    )
    .unwrap();

    for provider in ["codex", "claude"] {
        config
            .patch(&json!({"providers":{
                "codex":{"enabled":provider == "codex"},
                "claude":{"enabled":provider == "claude"}
            }}))
            .unwrap();
        let request = MetadataRequest {
            kind: MetadataKind::Title,
            cwd: fixture.cwd.to_str().unwrap().to_owned(),
            context: "Fix metadata assembly".into(),
            selection: Some(MetadataSelection {
                provider: provider.into(),
                model: Some("metadata-only".into()),
                thinking_option_id: None,
            }),
        };
        assert_eq!(
            generator.generate(request.clone()).await.unwrap()["title"],
            "Configured metadata"
        );
        if provider == "claude" {
            generator.shutdown();
            assert_eq!(
                generator.generate(request).await,
                Err(metadata::ports::generation::MetadataError::Cancelled)
            );
        }
    }
    assert!(!fixture.root.path().join("agents").exists());
    assert!(fixture.requests().iter().any(|request| {
        request["method"] == "thread/start" && request["params"]["ephemeral"] == true
    }));
    let claude_args: Vec<String> =
        serde_json::from_slice(&std::fs::read(fixture.cwd.join("claude-args.json")).unwrap())
            .unwrap();
    assert!(
        claude_args
            .iter()
            .any(|arg| arg == "--no-session-persistence")
    );
}
