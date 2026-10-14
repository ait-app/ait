use super::*;
use serde_json::json;

mod transport;

#[test]
fn credentials_and_diagnostics_never_expose_native_account_or_token_fields() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".credentials.json");
    std::fs::write(&path, json!({"claudeAiOauth":{"accessToken":"offline-fixture-token","refreshToken":"offline-refresh",
        "subscriptionType":"max","rateLimitTier":"default_max_20x"}}).to_string()).unwrap();
    let credentials = read_file(&path).unwrap().unwrap();
    assert_eq!(credentials.plan.as_deref(), Some("Max 20x"));
    assert!(!format!("{credentials:?}").contains("offline-fixture-token"));
    let status = diagnostic(Some(
        &json!({"loggedIn":true,"authMethod":"claude.ai","email":"private@example.test","token":"secret"}),
    ));
    assert!(!status.contains("private") && !status.contains("secret"));
    assert!(status.contains("Claude account login"));
    assert!(diagnostic(Some(&json!({"loggedIn":false}))).contains("not authenticated"));
    assert!(read_file(&root.path().join("missing")).unwrap().is_none());
    std::fs::write(&path, "not-json").unwrap();
    assert!(read_file(&path).unwrap().is_none());
}

#[test]
fn quota_reconciles_legacy_models_with_scoped_windows_without_losing_zero_or_surface_limits() {
    let response = json!({"five_hour":{"utilization":25,"resets_at":"2026-09-26T10:00:00Z"},
        "seven_day":{"utilization":"81"},"seven_day_opus":{"utilization":0},
        "limits":[{"kind":"weekly_scoped","percent":null,"scope":{"model":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":95,"scope":{"surface":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":2,"scope":{"model":{"id":"fable-pro"}}},
            {"kind":"weekly_scoped","percent":3,"scope":{"model":{"id":"fable_pro"}}},
            {"kind":"weekly_scoped","percent":"broken","scope":{"model":{"id":"ignored"}}}],
        "extra_usage":{"is_enabled":true}});
    let result = quota::project(&response, Some("Max")).unwrap();
    let windows = result["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 6);
    assert_eq!(windows[2]["usedPct"], 0.0);
    assert_eq!(windows[0]["shortLabel"], "5h");
    assert_eq!(windows[1]["shortLabel"], "wk");
    assert!(windows[2].get("shortLabel").is_none());
    assert_eq!(windows[3]["tone"], "danger");
    assert_ne!(windows[4]["id"], windows[5]["id"]);
    assert_eq!(result["details"][0]["value"], "Enabled");
    assert!(quota::project(&json!({"five_hour":{"utilization":"NaN"}}), None).is_err());
    let unknown = quota::project(
        &json!({"limits":[{"kind":"weekly_scoped","scope":{"model":{"id":"unknown"}}}]}),
        None,
    )
    .unwrap();
    assert!(unknown["windows"][0]["usedPct"].is_null());
    assert_eq!(unknown["windows"][0]["tone"], "default");
}

#[tokio::test]
async fn native_quota_respects_session_credentials_and_reports_expiry_without_network() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".credentials.json"),
        json!({"claudeAiOauth":{"accessToken":"offline-expired-token","expiresAt":1_700_000_000_000_i64}}).to_string()).unwrap();
    let mut client = ClaudeClient::new(root.path().join("unused-cli"));
    client.config_dir = Some(root.path().to_owned());
    client.environment = serde_json::from_value(json!({
        "CLAUDE_CODE_OAUTH_TOKEN":"", "ANTHROPIC_API_KEY":""
    }))
    .unwrap();
    let expired = client.native_usage().await.unwrap();
    assert_eq!(expired["problem"]["kind"], "expired");
    assert_eq!(expired["problem"]["expiresAt"], "2023-11-14T22:13:20.000Z");
    assert_eq!(expired["problem"]["refreshedBy"], "claude /login");
    assert!(!expired.to_string().contains("offline-expired-token"));

    client.environment = serde_json::from_value(json!({
        "CLAUDE_CODE_OAUTH_TOKEN":"", "ANTHROPIC_API_KEY":"offline-api-token"
    }))
    .unwrap();
    assert_eq!(
        client.native_usage().await.unwrap()["problem"]["kind"],
        "no_quota"
    );
    client.environment = serde_json::from_value(json!({
        "CLAUDE_CODE_OAUTH_TOKEN":"offline-session-token", "ANTHROPIC_API_KEY":""
    }))
    .unwrap();
    let credentials = client.credentials().await.unwrap().unwrap();
    assert_eq!(credentials.token.expose_secret(), "offline-session-token");
    assert!(credentials.expires_at.is_none());
    client.environment = serde_json::from_value(json!({
        "CLAUDE_CODE_OAUTH_TOKEN":"bad\nheader"
    }))
    .unwrap();
    assert!(matches!(
        client.credentials().await,
        Err(AgentSessionError::Rejected)
    ));
}
