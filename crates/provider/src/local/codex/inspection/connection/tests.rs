use super::*;
use crate::test_support::Fixture;

fn client(fixture: &Fixture) -> CodexClient {
    std::fs::write(
        fixture.root.path().join("auth.json"),
        r#"{"account":"first"}"#,
    )
    .unwrap();
    let mut client = fixture.client();
    client.environment = serde_json::from_value(json!({"CODEX_HOME":fixture.root.path()})).unwrap();
    client
}

#[tokio::test]
async fn repeated_preparation_and_polling_reuse_one_process_and_account_read() {
    let fixture = Fixture::new();
    let client = client(&fixture);
    for _ in 0..3 {
        let prepared = client
            .prepare_native_usage_in(fixture.cwd.to_str().unwrap())
            .await
            .unwrap();
        assert!(prepared.identity.is_some());
        assert_eq!(prepared.report.await.unwrap()["status"], "available");
    }
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "account/read")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "account/rateLimits/read")
            .count(),
        3
    );
    assert!(requests.iter().all(|r| r["pid"] == requests[0]["pid"]));
}

#[tokio::test]
async fn credential_and_config_changes_replace_native_process_and_identity_inspection() {
    let fixture = Fixture::new();
    let client = client(&fixture);
    let cwd = fixture.cwd.to_str().unwrap();
    client.native_usage_in(cwd).await.unwrap();
    let first = fingerprint(&client).await.unwrap();
    std::fs::write(
        fixture.root.path().join("auth.json"),
        r#"{"account":"second"}"#,
    )
    .unwrap();
    assert_ne!(fingerprint(&client).await.unwrap(), first);
    client.native_usage_in(cwd).await.unwrap();
    std::fs::write(
        fixture.root.path().join("config.toml"),
        "model = 'different'\n",
    )
    .unwrap();
    client.native_usage_in(cwd).await.unwrap();
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "account/read")
            .count(),
        3
    );
}

#[tokio::test]
async fn missing_unreadable_or_keyring_credentials_disable_fast_cache_identity() {
    let fixture = Fixture::new();
    let client = client(&fixture);
    assert!(fingerprint(&client).await.is_some());
    std::fs::write(
        fixture.root.path().join("config.toml"),
        "cli_auth_credentials_store = 'keyring'\n",
    )
    .unwrap();
    assert!(fingerprint(&client).await.is_none());
    std::fs::write(
        fixture.root.path().join("config.toml"),
        "cli_auth_credentials_store = 'file'\n",
    )
    .unwrap();
    assert!(fingerprint(&client).await.is_some());
    std::fs::remove_file(fixture.root.path().join("auth.json")).unwrap();
    assert!(fingerprint(&client).await.is_none());
    std::fs::write(
        fixture.root.path().join("auth.json"),
        vec![b'x'; 256 * 1024 + 1],
    )
    .unwrap();
    assert!(fingerprint(&client).await.is_none());
}

#[tokio::test]
async fn cancelled_quota_request_drops_process_and_rebuilds_on_next_read() {
    let fixture = Fixture::new();
    let client = client(&fixture);
    fixture.mode("slow-quota");
    let cwd = fixture.cwd.to_str().unwrap();
    let prepared = client.prepare_native_usage_in(cwd).await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), prepared.report)
            .await
            .is_err()
    );
    fixture.mode("normal");
    assert_eq!(
        client.native_usage_in(cwd).await.unwrap()["status"],
        "available"
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        2
    );
}

#[tokio::test]
async fn explicit_close_reaps_query_process_while_client_clones_remain_alive() {
    use crate::ports::agent_session::AgentClient;
    let fixture = Fixture::new();
    let client = client(&fixture);
    let retained = client.clone();
    client
        .native_usage_in(fixture.cwd.to_str().unwrap())
        .await
        .unwrap();
    let pid = fixture.requests().last().unwrap()["pid"].as_i64().unwrap();
    client.close_usage().await.unwrap();
    assert_reaped(pid);
    assert!(retained.usage_connection.account.lock().unwrap().is_none());
    retained.close_usage().await.unwrap();
}

#[tokio::test]
async fn worker_shutdown_reaps_host_query_process_before_retained_handle_is_dropped() {
    let fixture = Fixture::new();
    let client = client(&fixture);
    std::fs::write(
        fixture.program.with_extension("cwd"),
        fixture.cwd.to_str().unwrap(),
    )
    .unwrap();
    let (execution, _) = crate::test_support::worker_with_client(
        &fixture,
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    let report = execution
        .execute("provider.usage.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(report["providers"][0]["status"], "available");
    let pid = fixture.requests().last().unwrap()["pid"].as_i64().unwrap();
    execution.shutdown().await.unwrap();
    assert_reaped(pid);
    assert!(client.usage_connection.account.lock().unwrap().is_none());
    assert!(client.usage_connection.process.lock().await.is_none());
}

#[tokio::test]
async fn direct_and_resumed_sessions_keep_prepared_quota_transports_separate_from_host() {
    use crate::ports::agent_session::{AgentClient, AgentResumePurpose};
    let fixture = Fixture::new();
    let client = client(&fixture);
    let spec = fixture.spec();
    let mut direct = client.create_session(&spec).await.unwrap();
    let handle = direct.persistence().unwrap();
    let mut other_spec = spec.clone();
    let other_cwd = fixture.root.path().join("other-work");
    std::fs::create_dir(&other_cwd).unwrap();
    other_spec.cwd = other_cwd.to_str().unwrap().to_owned();
    let mut resumed = client
        .resume_session(&handle, &other_spec, AgentResumePurpose::History)
        .await
        .unwrap();
    // Interleave all three preparations before polling; sharing one pool would replace
    // the earlier prepared transport when the working directory changes.
    {
        let host = client
            .prepare_native_usage_in(spec.cwd.as_str())
            .await
            .unwrap();
        let direct_usage = direct.prepare_account_usage().await.unwrap();
        let resumed_usage = resumed.prepare_account_usage().await.unwrap();
        for report in [host.report, direct_usage.report, resumed_usage.report] {
            assert_eq!(report.await.unwrap()["status"], "available");
        }
    }
    let host_pid = fixture
        .requests()
        .iter()
        .rev()
        .find(|r| r["method"] == "account/rateLimits/read")
        .unwrap()["pid"]
        .as_i64()
        .unwrap();
    // Session shutdown must not close the host pool.
    direct.close().await.unwrap();
    resumed.close().await.unwrap();
    client.native_usage_in(&spec.cwd).await.unwrap();
    let requests = fixture.requests();
    assert_eq!(
        requests.last().unwrap()["pid"].as_i64().unwrap(),
        requests
            .iter()
            .find(|r| r["method"] == "account/rateLimits/read")
            .unwrap()["pid"]
            .as_i64()
            .unwrap()
    );
    client.close_usage().await.unwrap();
    assert_reaped(host_pid);
}

fn assert_reaped(pid: i64) {
    assert_eq!(
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap()),
            None
        ),
        Err(nix::errno::Errno::ESRCH),
        "native query process must be killed and reaped before close returns",
    );
}

#[tokio::test]
async fn cancelled_initialization_retains_child_for_explicit_reaping() {
    use crate::ports::agent_session::AgentClient;
    let fixture = Fixture::new();
    let client = client(&fixture);
    fixture.mode("timeout");
    let mut pending = Box::pin(client.prepare_native_usage_in(fixture.cwd.to_str().unwrap()));
    tokio::select! {
        _ = &mut pending => panic!("initialization should remain blocked"),
        result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if std::fs::read_to_string(fixture.cwd.join("native-requests.jsonl"))
                    .is_ok_and(|text| text.lines().any(|line| serde_json::from_str::<Value>(line).is_ok())) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }) => result.unwrap(),
    }
    drop(pending);
    let pid = fixture.requests().last().unwrap()["pid"].as_i64().unwrap();
    client.close_usage().await.unwrap();
    assert_reaped(pid);
}
