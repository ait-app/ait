use super::*;
use crate::ports::diagnostics::Category;
use serde_json::json;

#[test]
fn native_explanation_preserves_reason_status_and_retry_outcome() {
    let mut failure = Failure::default();
    let error = json!({"message":"unexpected status 403 Forbidden: insufficient balance"});
    let mut diagnostic = Diagnostic::new(Category::Quota, "notification");
    diagnostic.http_status = Some(403);
    diagnostic.native_code = Some(-32000);
    diagnostic.will_retry = Some(true);

    failure.observe(&error, diagnostic);
    assert_eq!(failure.message(), None);
    diagnostic.will_retry = Some(false);
    failure.observe(&error, diagnostic);
    let expected = "Codex (HTTP 403) (RPC -32000): unexpected status 403 Forbidden: insufficient balance\nCodex retries did not recover the request.";
    assert_eq!(failure.message(), Some(expected));
    assert_eq!(failure.take_message().as_deref(), Some(expected));
    assert_eq!(failure.message(), None);

    failure.clear();
    failure.observe(&error, diagnostic);
    assert!(
        failure
            .message()
            .unwrap()
            .ends_with("Codex will not retry automatically.")
    );
    diagnostic.will_retry = None;
    failure.observe(&error, diagnostic);
    assert!(
        failure
            .message()
            .unwrap()
            .ends_with("Codex will not retry automatically.")
    );
    failure.clear();
    assert_eq!(failure.message(), None);
    assert!(!failure.retried);
    assert!(!failure.stopped_retrying);
}

#[test]
fn missing_or_blank_message_uses_classification_without_erasing_a_previous_reason() {
    for error in [json!({}), json!({"message":"\0\t\n "})] {
        let mut failure = Failure::default();
        let diagnostic = Diagnostic::new(Category::ContextLimit, "result");

        failure.observe(&error, diagnostic);
        assert_eq!(
            failure.message(),
            Some(format!("Codex: {}", Category::ContextLimit.message()).as_str())
        );
        failure.observe(
            &json!({"message":"specific upstream explanation"}),
            diagnostic,
        );
        failure.observe(&error, diagnostic);
        assert_eq!(
            failure.message(),
            Some("Codex: specific upstream explanation")
        );
    }
}

#[test]
fn response_credentials_are_redacted_while_the_error_and_service_origin_remain_visible() {
    let message = concat!(
        "upstream rejected request; token=plain-secret; api_key='quoted secret'; ",
        "\"refresh_token\":\"json secret\"; authorization: Bearer bearer-secret; ",
        "Bearer standalone-secret; sk-proj-very-private-key; ghp_veryprivatetoken; ",
        "github_pat_veryprivatetoken; eyJhbGciOiJIUzI1NiJ9.cGF5bG9hZA.signature; ",
        "url: https://user:password@example.invalid/private-path?api_key=url-secret#fragment\0\n",
        "Cookie: session=cookie-secret; another=second-cookie-secret\n",
        "upstream request id: safe-request-id"
    );

    let sanitized = sanitize(message).unwrap();

    assert!(sanitized.contains("upstream rejected request"));
    assert!(sanitized.contains("https://example.invalid"));
    assert!(sanitized.contains("[redacted]"));
    assert!(sanitized.contains("safe-request-id"));
    for private in [
        "plain-secret",
        "quoted secret",
        "json secret",
        "bearer-secret",
        "standalone-secret",
        "very-private-key",
        "veryprivatetoken",
        "cGF5bG9hZA",
        "user:",
        "password@",
        "private-path",
        "url-secret",
        "fragment",
        "cookie-secret",
        "\0",
    ] {
        assert!(!sanitized.contains(private), "{private}");
    }
}

#[test]
fn serialized_api_errors_show_only_the_inner_explanation() {
    let mut failure = Failure::default();
    let nested = json!({"error":{"message":"Encrypted reasoning content could not be verified", "code":"invalid_encrypted_content"}, "request":{"prompt":"private prompt"}});

    failure.observe(
        &json!({"message":nested.to_string(), "additionalDetails":"private diagnostics"}),
        Diagnostic::new(Category::Unknown, "result"),
    );

    assert_eq!(
        failure.message(),
        Some("Codex: Encrypted reasoning content could not be verified")
    );
    assert!(!format!("{failure:?}").contains("private"));
    assert_eq!(
        native_message(&json!("plain native error")).unwrap(),
        "plain native error"
    );
    assert_eq!(
        native_message(&json!({"error":{"message":"nested native error"}})).unwrap(),
        "nested native error"
    );
}

#[test]
fn oversized_multibyte_errors_are_bounded_and_truncation_is_explicit() {
    let mut failure = Failure::default();
    let message = format!(
        "token={} {}",
        "s".repeat(MAX_NATIVE_BYTES),
        "界".repeat(MAX_NATIVE_BYTES)
    );
    let diagnostic = Diagnostic::new(Category::Unknown, "result");

    failure.observe(&json!({"message":message}), diagnostic);
    let sanitized = failure.message().unwrap();
    assert!(sanitized.len() <= 4096);
    assert!(sanitized.ends_with("[truncated]"));
    assert!(!sanitized.contains("ssss"));

    failure.clear();
    failure.observe(
        &json!({"message":"界".repeat(MAX_NATIVE_BYTES)}),
        diagnostic,
    );
    let sanitized = failure.message().unwrap();
    assert!(sanitized.len() <= 4096);
    assert!(sanitized.ends_with("[truncated]"));
}
