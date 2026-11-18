use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn serve(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
    serve_with_headers(status, body, "").await
}

async fn serve_with_headers(
    status: &str,
    body: &str,
    headers: &str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/usage", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
            assert!(request.len() < 8192);
        }
        let _ = socket.write_all(response.as_bytes()).await;
        String::from_utf8(request).unwrap()
    });
    (endpoint, task)
}

#[tokio::test]
async fn quota_http_is_read_only_bounded_and_refuses_auth_errors_or_redirects() {
    let token = SecretString::from("offline-test-token");
    for (status, body, success) in [
        (
            "200 OK",
            "{\"five_hour\":{\"utilization\":0}}".to_owned(),
            true,
        ),
        ("401 Unauthorized", "{}".to_owned(), false),
        ("403 Forbidden", "{}".to_owned(), false),
        ("302 Found", "{}".to_owned(), false),
        ("429 Too Many Requests", "{}".to_owned(), false),
        ("200 OK", "invalid".to_owned(), false),
        ("200 OK", "x".repeat(256 * 1024 + 1), false),
    ] {
        let (endpoint, task) = serve(status, &body).await;
        let result = fetch(
            &endpoint,
            &token,
            "ait/offline-test",
            Duration::from_secs(5),
        )
        .await;
        if status.starts_with("200") {
            assert_eq!(result.is_ok(), success, "{status}");
        } else {
            assert_eq!(
                result.unwrap()["httpStatus"],
                status[..3].parse::<u16>().unwrap()
            );
        }
        let request = task.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /usage http/1.1"));
        assert!(request.contains("authorization: bearer offline-test-token"));
        assert!(request.contains("anthropic-beta: oauth-2025-04-20"));
        assert!(request.contains("user-agent: ait/offline-test"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_auth_output_limits_timeout_and_preserves_logged_out_json() {
    for (script, success) in [
        ("printf '{\"loggedIn\":false}'; exit 1", true),
        ("exit 1", false),
        ("head -c 65537 /dev/zero", false),
    ] {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", script]);
        assert_eq!(
            output(command, Duration::from_secs(5)).await.is_ok(),
            success
        );
    }
    let mut command = tokio::process::Command::new("/bin/sleep");
    command.arg("5");
    assert!(output(command, Duration::from_millis(50)).await.is_err());
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let link = root.path().join("linked");
    std::fs::write(&source, "x".repeat(65537)).unwrap();
    assert!(read_file(&source).is_err());
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(read_file(&link).is_err());
}

#[tokio::test]
async fn native_usage_projects_authenticated_quota_and_classifies_rejected_credentials() {
    for status in [
        "200 OK",
        "401 Unauthorized",
        "403 Forbidden",
        "500 Internal Server Error",
    ] {
        let (endpoint, task) = serve(status, r#"{"five_hour":{"utilization":25}}"#).await;
        let root = tempfile::tempdir().unwrap();
        let mut client = ClaudeClient::new(root.path().join("unused-cli"));
        client.environment =
            serde_json::from_value(json!({"CLAUDE_CODE_OAUTH_TOKEN":"offline-token"})).unwrap();

        let result = client.native_usage_at(&endpoint).await;

        if status.starts_with("200") {
            assert_eq!(result.unwrap()["windows"][0]["usedPct"], 25.0);
        } else if status.starts_with("500") {
            assert_eq!(result, Err(AgentSessionError::Failed));
        } else {
            let report = result.unwrap();
            assert_eq!(report["problem"]["kind"], "rejected");
            assert_eq!(
                report["problem"]["status"],
                status[..3].parse::<u16>().unwrap()
            );
            assert!(!report.to_string().contains("offline-token"));
        }
        assert!(task.await.unwrap().contains("Bearer offline-token"));
    }
}

#[tokio::test]
async fn native_usage_reports_rate_limiting_without_exposing_the_upstream_body() {
    let (endpoint, task) = serve(
        "429 Too Many Requests",
        r#"{"error":{"type":"rate_limit_error","message":"private-account-detail"}}"#,
    )
    .await;
    let root = tempfile::tempdir().unwrap();
    let mut client = ClaudeClient::new(root.path().join("unused-cli"));
    client.environment =
        serde_json::from_value(json!({"CLAUDE_CODE_OAUTH_TOKEN":"offline-token"})).unwrap();

    let result = client.native_usage_at(&endpoint).await;
    task.await.unwrap();

    let report =
        result.expect("HTTP 429 should report throttling instead of a generic read failure");
    assert_eq!(report["status"], "error");
    assert_eq!(
        report["error"],
        "Claude usage is temporarily rate limited (HTTP 429). Please try again later."
    );
    assert!(report["nextRefreshAt"].as_str().is_some());
    assert!(!report.to_string().contains("private-account-detail"));
    assert!(!report.to_string().contains("offline-token"));
}

#[tokio::test]
async fn quota_http_preserves_the_server_retry_delay() {
    let (endpoint, task) =
        serve_with_headers("429 Too Many Requests", "{}", "Retry-After: 120\r\n").await;
    let started = chrono::Utc::now();

    let response = fetch(
        &endpoint,
        &SecretString::from("offline-token"),
        "ait/offline-test",
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    task.await.unwrap();

    let retry_at =
        chrono::DateTime::parse_from_rfc3339(response["nextRefreshAt"].as_str().unwrap()).unwrap();
    assert!(retry_at.signed_duration_since(started).num_seconds() >= 120);
    assert!(
        retry_at
            .signed_duration_since(chrono::Utc::now())
            .num_seconds()
            <= 120
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_usage_sends_the_installed_claude_client_header() {
    use std::os::unix::fs::PermissionsExt as _;

    let (endpoint, task) = serve("200 OK", r#"{"five_hour":{"utilization":25}}"#).await;
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("claude");
    std::fs::write(
        &program,
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 1\nprintf '2.1.999 (Claude Code)\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = ClaudeClient::new(program);
    client.environment =
        serde_json::from_value(json!({"CLAUDE_CODE_OAUTH_TOKEN":"offline-token"})).unwrap();

    let report = client.native_usage_at(&endpoint).await.unwrap();
    let request = task.await.unwrap().to_ascii_lowercase();

    assert_eq!(report["status"], "available");
    assert!(request.contains("user-agent: claude-cli/2.1.999 (external, ait/"));
    assert!(request.contains("content-type: application/json"));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn keychain_uses_safe_accounts_and_recovers_with_an_unscoped_lookup() {
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_keychain.py");
    for (user, expected_plan) in [
        ("fixture.user", "Pro"),
        ("bad username", "Max"),
        ("", "Max"),
    ] {
        let credentials = keychain_at(&program, user).await.unwrap().unwrap();
        assert_eq!(credentials.plan.as_deref(), Some(expected_plan));
        assert_eq!(
            credentials.token.expose_secret(),
            "offline-keychain-fixture"
        );
        assert!(!format!("{credentials:?}").contains("offline-keychain-fixture"));
    }
    let root = tempfile::tempdir().unwrap();
    assert!(
        keychain_at(&root.path().join("missing"), "fixture.user")
            .await
            .unwrap()
            .is_none()
    );
}
