use std::process::Command;

use secrecy::ExposeSecret;

use super::*;

const CREDENTIAL_VARIABLE: &str = "AIT_SERVER_CREDENTIAL_DAEMON_COVERAGE_TEST";
const CHILD_CASE: &str = "AIT_DAEMON_TEST_BOT_CASE";

#[test]
fn environment_credentials_validate_references_and_bound_secret_values() {
    if let Ok(case) = std::env::var(CHILD_CASE) {
        let result = Environment.resolve(&format!("env:{CREDENTIAL_VARIABLE}"));
        if case == "valid" {
            let secret = result.unwrap();
            assert_eq!(secret.expose_secret(), "fixture.bot.credential");
            assert!(!format!("{secret:?}").contains("fixture.bot.credential"));
        } else {
            assert_eq!(result.unwrap_err(), ErrorCode::BotIo);
        }
        return;
    }
    for reference in ["", "file:credentials", "env:", "env:bad-name"] {
        assert_eq!(
            Environment.resolve(reference).unwrap_err(),
            ErrorCode::InvalidMessage
        );
    }
    for (case, value) in [
        ("valid", Some("fixture.bot.credential".to_owned())),
        ("missing", None),
        ("empty", Some(String::new())),
        ("oversized", Some("x".repeat(4097))),
        ("control", Some("fixture\ncredential".to_owned())),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "host::bot::tests::environment_credentials_validate_references_and_bound_secret_values",
            ])
            .env(CHILD_CASE, case)
            .env_remove(CREDENTIAL_VARIABLE);
        if let Some(value) = value {
            child.env(CREDENTIAL_VARIABLE, value);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "case {case}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn startup_ignores_and_discards_malformed_legacy_store_root() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("bots");
    std::fs::create_dir_all(legacy.join("conversations")).unwrap();
    std::fs::write(legacy.join("catalog.json"), "{not json").unwrap();
    std::fs::write(legacy.join("conversations/truncated.json"), "{\"id\"").unwrap();
    let server = crate::host::Server::bind(crate::host::tests::config(root.path()))
        .await
        .unwrap();
    for _ in 0..100 {
        if !legacy.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!legacy.exists(), "legacy Bot store must be discarded");
    server.api.begin_shutdown();
    server.api.wait_closed().await;
}
