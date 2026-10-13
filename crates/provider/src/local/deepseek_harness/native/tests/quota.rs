use super::fixture::Fixture;
use crate::{
    local::deepseek_harness::PROVIDER, ports::agent_session::AgentClient,
    service::agent_manager::AgentManager,
};
use serde_json::json;

#[tokio::test]
async fn deepseek_balance_is_read_without_creating_or_prompting_a_session() {
    let fixture = Fixture::new().await;
    let report = fixture.client.usage().await.unwrap();
    assert_eq!(report["status"], "available");
    assert_eq!(report["windows"], json!([]));
    assert_eq!(report["balances"][0]["remaining"], 12.34);
    assert_eq!(report["balances"][0]["unit"], "cny");
    assert_eq!(report["balances"][1]["remaining"], 0.5);
    assert_eq!(report["balances"][1]["unit"], "usd");
    assert!(report["balances"][0].get("limit").is_none());
    let requests = fixture.requests("account/getBalance");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["client"]["version"], env!("CARGO_PKG_VERSION"));
    for method in [
        "session/create",
        "session/modelCatalog",
        "session/prompt",
        "commands/execute",
    ] {
        assert!(fixture.requests(method).is_empty(), "{method}");
    }
}

#[tokio::test]
async fn balance_refresh_uses_the_host_cache_and_handles_signed_out_accounts() {
    let fixture = Fixture::new().await;
    let directory = tempfile::tempdir().unwrap();
    let mut manager = AgentManager::new(Box::new(
        persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
            directory.path().join("agents.json"),
        ),
    ));
    manager
        .register_client(Box::new(fixture.client.clone()))
        .unwrap();
    let first = manager.usage(None, Some(PROVIDER), true).await.unwrap();
    assert_eq!(first["providers"][0]["status"], "available");
    fixture.set_balance(json!(null));
    let cached = manager.usage(None, Some(PROVIDER), false).await.unwrap();
    assert_eq!(first["providers"], cached["providers"]);
    assert_eq!(fixture.requests("account/getBalance").len(), 1);
    let refreshed = manager.usage(None, Some(PROVIDER), true).await.unwrap();
    assert_eq!(refreshed["providers"][0]["status"], "unavailable");
    assert!(
        refreshed["providers"][0]["problem"]["detail"]
            .as_str()
            .unwrap()
            .contains("DeepSeek")
    );
    assert_eq!(fixture.requests("account/getBalance").len(), 2);
}

#[tokio::test]
async fn live_account_queries_keep_the_session_environment_and_do_not_fall_back_to_host() {
    use crate::service::agent_manager::AgentRegistration;
    let host = Fixture::new().await;
    let session = Fixture::new().await;
    session.set_balance(
        json!({"status":"ready","value":[{"currency":"CNY","balance":"9.25"}],"bonusWallets":[]}),
    );
    let directory = tempfile::tempdir().unwrap();
    let mut manager = AgentManager::new(Box::new(
        persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
            directory.path().join("agents.json"),
        ),
    ));
    manager
        .register_client(Box::new(host.client.clone()))
        .unwrap();
    manager
        .create_with_environment(
            "quota-agent",
            &session.spec,
            AgentRegistration::default(),
            &session.client.environment,
        )
        .await
        .unwrap();
    let report = manager
        .usage(Some("quota-agent"), Some(PROVIDER), true)
        .await
        .unwrap();
    assert_eq!(report["providers"][0]["balances"][0]["remaining"], 9.25);
    assert!(host.requests("account/getBalance").is_empty());
    assert_eq!(session.requests("session/create").len(), 1);
    assert!(session.requests("session/prompt").is_empty());
    manager.close_all().await.unwrap();
    let stopped = manager
        .usage(Some("quota-agent"), Some(PROVIDER), true)
        .await
        .unwrap();
    assert_eq!(stopped["providers"][0]["status"], "unavailable");
    assert_eq!(session.requests("account/getBalance").len(), 1);
    assert!(host.requests("account/getBalance").is_empty());
}

#[tokio::test]
async fn native_failures_are_safe_and_only_missing_capabilities_are_unavailable() {
    use std::time::Duration;
    for scenario in [
        "404",
        "401",
        "500",
        "malformed",
        "missing",
        "correlation",
        "rejected",
        "large",
        "timeout",
    ] {
        let mut fixture = Fixture::new().await;
        fixture.fail("account/getBalance", scenario);
        if scenario == "timeout" {
            fixture.client.deadline = Duration::from_millis(150);
        }
        let report = tokio::time::timeout(Duration::from_secs(3), fixture.client.usage())
            .await
            .unwrap();
        if scenario == "404" {
            let report = report.unwrap();
            assert_eq!(report["status"], "unavailable");
            assert!(
                report["problem"]["detail"]
                    .as_str()
                    .unwrap()
                    .contains("API")
            );
        } else {
            assert!(report.is_err(), "{scenario}");
        }
    }
    let fixture = Fixture::new().await;
    let report = fixture
        .client
        .clone()
        .with_acp_profile()
        .usage()
        .await
        .unwrap();
    assert_eq!(report["status"], "unavailable");
    assert!(
        report["problem"]["detail"]
            .as_str()
            .unwrap()
            .contains("ACP")
    );
    assert!(fixture.requests("account/getBalance").is_empty());
}

#[tokio::test]
async fn deepseek_key_resolution_follows_native_sources_custom_references_and_live_store_changes() {
    use super::super::{quota::credentials::Context, runtime::Runtime};
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    let mut fixture = Fixture::new().await;
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    tokio::fs::create_dir(&home).await.unwrap();
    let keyfile = home.join(".credentials.yaml");
    tokio::fs::write(&keyfile, "version: 1\nrefs:\n  CUSTOM_DSH_KEY: stored\n")
        .await
        .unwrap();
    tokio::fs::write(directory.path().join(".env"), "CUSTOM_DSH_KEY=project\n")
        .await
        .unwrap();
    tokio::fs::write(home.join(".env"), "CUSTOM_DSH_KEY=user\n")
        .await
        .unwrap();
    let mut environment = fixture
        .client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    environment.insert("DSH_HOME".into(), home.to_str().unwrap().into());
    environment.insert("CUSTOM_DSH_KEY".into(), "launch".into());
    fixture.client.environment = environment.try_into().unwrap();
    fixture.reply(
        "settings/describe",
        json!({"namespaces":[{"ns":"llm-deepseek","value":{"apiKeyEnv":"CUSTOM_DSH_KEY"}}]}),
    );
    let cwd = directory.path().to_str().unwrap();
    let context = Context::capture(&fixture.client, cwd).await.unwrap();
    let mut runtime = Runtime::open(&fixture.client, cwd).await.unwrap();
    tokio::fs::write(directory.path().join(".env"), "CUSTOM_DSH_KEY=changed\n")
        .await
        .unwrap();
    for (source, expected) in [
        ("env", "launch"),
        ("file", "stored"),
        ("project-env", "project"),
        ("user-env", "user"),
    ] {
        fixture.reply(
            "credentials/describe",
            json!({"CUSTOM_DSH_KEY":{"configured":true,"source":source}}),
        );
        assert_eq!(
            context
                .resolve(&runtime.api)
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            expected
        );
    }
    tokio::fs::write(&keyfile, "version: 1\nrefs:\n  CUSTOM_DSH_KEY: updated\n")
        .await
        .unwrap();
    fixture.reply(
        "credentials/describe",
        json!({"CUSTOM_DSH_KEY":{"configured":true,"source":"file"}}),
    );
    assert_eq!(
        context
            .resolve(&runtime.api)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        "updated"
    );
    fixture.reply("settings/describe",json!({"namespaces":[{"ns":"llm-deepseek","value":{"apiKeyEnv":"CUSTOM_DSH_KEY","baseURL":"https://other.test"}}]}));
    assert!(context.resolve(&runtime.api).await.is_err());
    assert!(fixture.requests("account/getBalance").is_empty());
    assert!(fixture.requests("session/create").is_empty());
    runtime.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN and a configured DeepSeek API key; only reads official balance"]
async fn installed_deepseek_api_key_balance_without_model_execution() {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let client = crate::local::deepseek_harness::DeepSeekHarnessClient::new(binary.into());
    let report = client
        .usage()
        .await
        .expect("native account balance query failed");
    assert_eq!(
        report["status"], "available",
        "native balance unavailable; raw report withheld"
    );
    assert_eq!(report["sourceLabel"], "DSH DeepSeek API balance");
    assert!(!report["balances"].as_array().unwrap().is_empty());
    assert!(report["windows"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn custom_store_paths_missing_keys_and_unknown_native_sources_never_use_another_key() {
    use super::super::{quota::credentials::Context, runtime::Runtime};
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    let mut fixture = Fixture::new().await;
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profile.yaml");
    let keyfile = directory.path().join("custom.yaml");
    tokio::fs::write(&config,"- id: custom-deepseek\n  name: '@deepseek-ai/dsh-llm-deepseek-api-key'\n- id: custom-credentials\n  name: '@deepseek-ai/dsh-credentials-local'\n  config:\n    path: custom.yaml\n").await.unwrap();
    tokio::fs::write(
        &keyfile,
        "version: 1\nrefs:\n  CUSTOM_DSH_KEY: custom-store\n",
    )
    .await
    .unwrap();
    let mut environment = fixture
        .client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    environment.insert(
        "AIT_DSH_FIXTURE_CONFIG".into(),
        config.to_str().unwrap().into(),
    );
    environment.insert("CUSTOM_DSH_KEY".into(), "wrong-source-key".into());
    fixture.client.environment = environment.try_into().unwrap();
    fixture.reply(
        "settings/describe",
        json!({"namespaces":[{"ns":"custom-deepseek","value":{"apiKeyEnv":"CUSTOM_DSH_KEY"}}]}),
    );
    fixture.reply(
        "credentials/describe",
        json!({"CUSTOM_DSH_KEY":{"configured":true,"source":"file"}}),
    );
    let cwd = directory.path().to_str().unwrap();
    let context = Context::capture(&fixture.client, cwd).await.unwrap();
    let mut runtime = Runtime::open(&fixture.client, cwd).await.unwrap();
    assert_eq!(
        context
            .resolve(&runtime.api)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        "custom-store"
    );
    tokio::fs::write(&keyfile, "version: 1\nrefs: {}\n")
        .await
        .unwrap();
    assert!(context.resolve(&runtime.api).await.is_err());
    for info in [
        json!({"configured":true,"source":"unknown"}),
        json!({"configured":true}),
        json!({}),
    ] {
        fixture.reply("credentials/describe", json!({"CUSTOM_DSH_KEY":info}));
        assert!(context.resolve(&runtime.api).await.is_err());
    }
    fixture.reply(
        "credentials/describe",
        json!({"CUSTOM_DSH_KEY":{"configured":false}}),
    );
    assert!(context.resolve(&runtime.api).await.unwrap().is_none());
    runtime.close().await.unwrap();
}
