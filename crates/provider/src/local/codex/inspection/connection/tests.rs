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
