use super::*;
use serde_json::json;

#[test]
fn classifies_common_failures_without_retaining_native_text() {
    for (text, expected) in [
        ("invalid_api_key secret", Category::Authentication),
        ("permission denied /private/path", Category::Permission),
        ("too many requests", Category::RateLimit),
        ("insufficient_quota", Category::Quota),
        ("Selected model is at capacity", Category::Capacity),
        ("model_not_found", Category::Model),
        ("prompt is too long", Category::ContextLimit),
        ("connection reset", Category::Network),
        ("request timed out", Category::Timeout),
        ("secret provider diagnostic", Category::Unknown),
    ] {
        assert_eq!(classify(text), expected);
        assert!(!format!("{:?}", native(&json!({"message":text}), "test")).contains("secret"));
        assert!(!expected.message().contains("/private/path"));
        assert!(!expected.code().is_empty());
    }
    for category in [Category::Closed, Category::Protocol, Category::Unavailable] {
        assert!(!category.message().is_empty());
        assert!(!category.code().is_empty());
    }
}

#[test]
fn structured_codes_take_precedence_and_status_does_not_misclassify_quota() {
    let diagnostic = native(
        &json!({"code":-32000,"message":"rate limit",
        "data":{"codexErrorInfo":{"usageLimitExceeded":{}}},"statusCode":429}),
        "rpc",
    );
    assert_eq!(diagnostic.category, Category::Quota);
    assert_eq!(diagnostic.native_code, Some(-32000));
    assert_eq!(diagnostic.http_status, Some(429));
    assert_eq!(diagnostic.will_retry, None);
    for (status, expected) in [
        (401, Category::Authentication),
        (403, Category::Permission),
        (429, Category::RateLimit),
        (504, Category::Timeout),
        (500, Category::Unknown),
    ] {
        assert_eq!(
            native(&json!({"httpStatusCode":status}), "rpc").category,
            expected
        );
    }
    assert_eq!(
        native(
            &json!({"data":{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":503}}}}),
            "rpc"
        )
        .http_status,
        Some(503)
    );
}

#[test]
fn specific_failure_survives_closure_and_retryable_errors_do_not_become_terminal() {
    let diagnostics = Diagnostics::new("codex");
    let mut retrying = Diagnostic::new(Category::Network, "notification");
    retrying.will_retry = Some(true);
    diagnostics.observe(retrying, "turn");
    assert_eq!(diagnostics.failure(), None);
    let failure = Diagnostic::new(Category::Authentication, "rpc");
    diagnostics.observe(failure, "turn");
    diagnostics.observe(Diagnostic::new(Category::Closed, "stdout"), "read");
    assert_eq!(diagnostics.failure(), Some(failure));
    diagnostics.clear();
    assert_eq!(diagnostics.failure(), None);
}

#[test]
fn repeated_errors_are_counted_and_flushed_with_constant_memory() {
    let diagnostics = Diagnostics::new("claude");
    let failure = Diagnostic::new(Category::RateLimit, "notification");
    for _ in 0..1000 {
        diagnostics.observe(failure, "turn");
    }
    assert_eq!(diagnostics.state.lock().unwrap().suppressed, 999);
    diagnostics.flush();
    assert_eq!(diagnostics.state.lock().unwrap().suppressed, 0);
    assert!(diagnostics.state.lock().unwrap().logged.is_none());
}

#[tokio::test]
async fn stderr_draining_is_bounded_and_does_not_retain_credentials() {
    let diagnostics = Diagnostics::new("opencode");
    let text = format!(
        "{}\nauthentication failed token=secret\n",
        "界".repeat(MAX_TEXT)
    );
    diagnostics.clone().drain(text.as_bytes()).await;
    assert_eq!(
        diagnostics.failure().unwrap().category,
        Category::Authentication
    );
    assert!(!format!("{diagnostics:?}").contains("secret"));
    assert_eq!(classify(&"界".repeat(MAX_TEXT)), Category::Unknown);
}
