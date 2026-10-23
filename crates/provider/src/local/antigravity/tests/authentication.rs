use super::*;

fn sign_out(directory: &TempDir, client: &mut AntigravityClient) {
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        ("AGY_FIXTURE_SCENARIO".to_owned(), "signed-out".to_owned()),
        (
            "AGY_FIXTURE_LOG".to_owned(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
    ]))
    .unwrap();
}

#[tokio::test]
async fn repeated_usage_refreshes_stop_before_interactive_authentication() {
    let (directory, mut client, _) = fixture();
    sign_out(&directory, &mut client);

    for _ in 0..3 {
        assert!(client.usage().await.is_err());
    }

    let calls = logs(&directory);
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call["args"] == json!(["models"])));
}

#[tokio::test]
async fn signed_out_creation_and_restore_stop_before_interactive_authentication() {
    let (directory, mut client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let initial_calls = logs(&directory).len();
    sign_out(&directory, &mut client);

    assert!(client.create_session(&spec).await.is_err());
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );

    let calls = logs(&directory);
    let attempts = &calls[initial_calls..];
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts
            .iter()
            .all(|call| call["args"] == json!(["models"]))
    );
}

#[tokio::test]
async fn native_login_recovery_allows_a_later_usage_refresh() {
    let (directory, mut client, _) = fixture();
    let signed_in_environment = client.environment.clone();
    sign_out(&directory, &mut client);
    assert!(client.usage().await.is_err());

    client.environment = signed_in_environment;
    let report = client.usage().await.unwrap();

    assert_eq!(report["status"], "available");
    let calls = logs(&directory);
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["args"], json!(["models"]));
    assert_eq!(calls[1]["args"], json!(["models"]));
    assert_eq!(
        calls[2]["args"],
        json!(["--print", "/usage", "--output-format", "json"])
    );
}
