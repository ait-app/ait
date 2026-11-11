use super::*;

#[cfg(unix)]
use crate::service::summary_generation::test_config::Configuration;
#[cfg(unix)]
use domain::summary::{SummaryKind, SummaryRequest, SummarySelection};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
#[cfg(unix)]
use serde_json::json;

#[test]
fn registration_preserves_the_catalog_without_startup_writes_and_rejects_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("absent");
    let mut manager = AgentManager::new(Box::new(FileBackedAgentRuntimeRegistry::new(
        data.join("agents.json"),
    )));

    let providers = Providers::new(&data);
    assert_eq!(
        providers
            .clients
            .iter()
            .map(|client| client.provider())
            .collect::<Vec<_>>(),
        [
            "codex",
            "claude",
            "antigravity",
            "opencode",
            "deepseek-harness",
            "pi",
            "omp",
            "minimax-code",
        ]
    );
    providers.register(&mut manager).unwrap();
    for provider in [
        "codex",
        "claude",
        "antigravity",
        "opencode",
        "deepseek-harness",
        "pi",
        "omp",
        "minimax-code",
    ] {
        manager.validate_provider(provider).unwrap();
    }
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
    for client in &providers.summary_clients {
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
async fn configured_codex_and_claude_generate_summary_without_foreground_agents() {
    let fixture = crate::test_support::Fixture::new();
    let config = Arc::new(Configuration::default());
    let providers = Providers::configured(fixture.root.path(), |name| {
        Some(match name {
            "AIT_SERVER_CODEX_BIN" => fixture.program.clone().into_os_string(),
            "AIT_SERVER_CLAUDE_BIN" => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude_code.py")
                .into_os_string(),
            _ => fixture.root.path().join(name).into_os_string(),
        })
    });
    let generator = providers.summary_generator(config.clone());
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
            },"metadataGeneration":{"providers":[{"provider":provider,"model":"metadata-only"}]}}))
            .unwrap();
        let request = SummaryRequest {
            kind: SummaryKind::Title,
            cwd: fixture.cwd.to_str().unwrap().to_owned(),
            context: "Fix metadata assembly".into(),
            selection: Some(SummarySelection {
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
                Err(domain::summary::SummaryError::Cancelled)
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

#[cfg(unix)]
#[tokio::test]
async fn summary_generators_record_their_calls_in_the_installed_host_ledger() {
    use crate::ports::usage_ledger::{
        CallOutcome, CallSource, UsageGroup, UsageLedger, UsageQuery,
    };

    let fixture = crate::test_support::Fixture::new();
    let data = tempfile::tempdir().unwrap();
    let ledger = Arc::new(SqliteUsageLedger::open(data.path()).unwrap());
    let config = Arc::new(Configuration::default());
    config
        .patch(&json!({"metadataGeneration":{"providers":[{"provider":"codex","model":"m"}]}}))
        .unwrap();
    let generator = Providers::configured(fixture.root.path(), |name| {
        Some(if name == "AIT_SERVER_CODEX_BIN" {
            fixture.program.clone().into_os_string()
        } else {
            fixture.root.path().join(name).into_os_string()
        })
    })
    .with_usage_ledger(ledger.clone())
    .summary_generator(config);
    std::fs::write(
        fixture.cwd.join("metadata-response.json"),
        r#"{"title":"Recorded title"}"#,
    )
    .unwrap();

    generator
        .generate(SummaryRequest {
            kind: SummaryKind::Title,
            cwd: fixture.cwd.to_str().unwrap().to_owned(),
            context: "Record the generation".into(),
            selection: None,
        })
        .await
        .unwrap();
    let summary = ledger
        .summarize(UsageQuery {
            from: 0,
            to: chrono::Utc::now().timestamp_millis() + 1,
            group_by: vec![UsageGroup::Source, UsageGroup::Outcome],
        })
        .await
        .unwrap();

    let rows: Vec<_> = summary
        .rows
        .iter()
        .map(|row| (row.key.source, row.key.outcome, row.calls))
        .collect();
    assert_eq!(
        rows,
        [(Some(CallSource::Summary), Some(CallOutcome::Completed), 1)]
    );
}
