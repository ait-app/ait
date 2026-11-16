use super::*;
use serde_json::json;

#[test]
fn native_reason_overrides_an_earlier_transport_fallback() {
    let diagnostics = Diagnostics::new("Oh My Pi");
    diagnostics.observe(Diagnostic::new(Category::Closed, "stdout"), "read");
    diagnostics.observe_native(
        &json!({"message":"Upstream shard unavailable api_key=fixture-secret","statusCode":503}),
        "result",
        "turn",
    );
    diagnostics.observe_failure(
        Diagnostic::new(Category::Protocol, "event"),
        "Invalid or missing OMP field: message.timestamp.",
        "project",
    );
    diagnostics.observe(Diagnostic::new(Category::Closed, "stdout"), "read");

    let message = diagnostics.message().unwrap();
    assert!(message.contains("Upstream shard unavailable"));
    assert!(message.contains("HTTP 503"));
    assert!(!message.contains("fixture-secret"));
    assert!(!message.contains("message.timestamp"));
    assert!(!format!("{diagnostics:?}").contains("Upstream shard"));
}

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
        assert_ne!(expected.code(), "");
    }
    for category in [Category::Closed, Category::Protocol, Category::Unavailable] {
        assert_ne!(category.message(), "");
        assert_ne!(category.code(), "");
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
fn codex_http_wrapper_preserves_specific_upstream_failures() {
    for (status, message, expected) in [
        (
            403,
            "unexpected status 403 Forbidden: insufficient balance",
            Category::Quota,
        ),
        (403, "Access blocked by Cloudflare", Category::Permission),
        (401, "upstream request failed", Category::Authentication),
        (429, "upstream request failed", Category::RateLimit),
        (504, "upstream request failed", Category::Timeout),
        (503, "upstream request failed", Category::Network),
    ] {
        for nested in [false, true] {
            let info = json!({"httpConnectionFailed":{"httpStatusCode":status}});
            let mut error = json!({"message":format!("{message} token=secret")});
            if nested {
                error["data"] = json!({"codexErrorInfo":info});
            } else {
                error["codexErrorInfo"] = info;
            }
            let diagnostic = native(&error, "result");

            assert_eq!(diagnostic.category, expected, "{status}: {message}");
            assert_eq!(diagnostic.http_status, Some(status));
            assert!(!format!("{diagnostic:?}").contains("secret"));
        }
    }
}

#[test]
fn incomplete_upstream_responses_are_distinct_from_connection_failures() {
    for info in [
        json!("other"),
        json!({"responseStreamDisconnected":{"httpStatusCode":null}}),
    ] {
        let diagnostic = native(
            &json!({
                "message":"stream disconnected before completion: stream closed before response.completed secret",
                "codexErrorInfo":info
            }),
            "result",
        );

        assert_eq!(diagnostic.category, Category::IncompleteResponse);
        assert_eq!(diagnostic.category.code(), "incomplete_response");
        assert!(!diagnostic.category.message().contains("secret"));
        assert!(!diagnostic.category.message().contains("Check your network"));
    }
    assert_eq!(
        native(
            &json!({
                "codexErrorInfo":"contextWindowExceeded", "message":"stream disconnected"
            }),
            "result"
        )
        .category,
        Category::ContextLimit
    );
    assert_eq!(
        native(
            &json!({
                "codexErrorInfo":"usageLimitExceeded", "message":"stream disconnected"
            }),
            "result"
        )
        .category,
        Category::Quota
    );
    assert_eq!(
        native(
            &json!({
                "codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":400}},
                "message":"context_length_exceeded"
            }),
            "result"
        )
        .category,
        Category::ContextLimit
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
fn codex_model_catalog_timeout_is_logged_without_becoming_a_turn_failure() {
    let diagnostics = Diagnostics::new("codex");
    diagnostics.line(
        b"2026-10-14T15:32:31Z ERROR codex_models_manager::manager: list_models{refresh_strategy=online}: failed to refresh available models: request timed out",
    );

    assert_eq!(diagnostics.failure(), None);
    let logged = diagnostics.state.lock().unwrap().logged.unwrap().0;
    assert_eq!(logged.category, Category::Timeout);
    assert_eq!(logged.source, "model_catalog");
    assert_eq!(logged.will_retry, None);

    diagnostics.line(b"ERROR codex_core::stream_events_utils: request timed out");
    assert_eq!(diagnostics.failure().unwrap().category, Category::Timeout);
    assert_eq!(diagnostics.failure().unwrap().source, "stderr");
}

#[test]
fn background_catalog_failure_does_not_overwrite_an_existing_execution_failure() {
    let diagnostics = Diagnostics::new("codex");
    let failure = Diagnostic::new(Category::Authentication, "rpc");
    diagnostics.observe(failure, "turn/start");

    diagnostics.line(
        b"ERROR codex_models_manager::manager: failed to refresh available models: request timed out",
    );

    assert_eq!(diagnostics.failure(), Some(failure));
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

#[test]
fn native_details_survive_closure_and_stay_out_of_debug_output() {
    let diagnostics = Diagnostics::new("claude");
    diagnostics.observe_native(&json!({"message":"Failed to authenticate: OAuth session expired and could not be refreshed", "status":401}), "result", "turn");
    diagnostics.observe(Diagnostic::new(Category::Closed, "stdout"), "read");
    diagnostics.observe(Diagnostic::new(Category::Timeout, "transport"), "close");
    let message = diagnostics.message().unwrap();
    assert!(
        message
            .starts_with("Claude Code (HTTP 401): Failed to authenticate: OAuth session expired")
    );
    assert!(message.contains("Sign in again"));
    assert_eq!(
        diagnostics.failure().unwrap().category,
        Category::Authentication
    );
    assert!(!format!("{diagnostics:?}").contains("OAuth"));
    diagnostics.clear();
    assert_eq!(diagnostics.message(), None);
    assert_eq!(diagnostics.failure(), None);
}

#[test]
fn structured_error_envelopes_preserve_specific_causes_and_sanitize_details() {
    for (error, category, detail) in [
        (
            json!({"code":-32000,"data":{"message":"account locked api_key=private-key"}}),
            Category::Unknown,
            "account locked",
        ),
        (
            json!({"status":429,"message":json!({"error":{"message":"insufficient balance token=private-key"},"request":{"prompt":"private prompt"}}).to_string()}),
            Category::Quota,
            "insufficient balance",
        ),
        (
            json!({"type":"authentication_failed", "message":"OAuth session expired"}),
            Category::Authentication,
            "OAuth session expired",
        ),
        (
            json!({"status":402, "message":"Payment required"}),
            Category::Quota,
            "Payment required",
        ),
        (
            json!({"message":"model gateway unavailable token=private-key"}),
            Category::Unknown,
            "model gateway unavailable",
        ),
    ] {
        let diagnostics = Diagnostics::new("opencode");
        diagnostics.observe_native(&error, "rpc", "session/prompt");
        assert_eq!(diagnostics.failure().unwrap().category, category);
        let message = diagnostics.message().unwrap();
        assert!(message.contains(detail), "{message}");
        assert!(!message.contains("private-key"), "{message}");
        assert!(!message.contains("private prompt"), "{message}");
    }
}

#[test]
fn retrying_errors_do_not_create_final_explanations_and_restored_controls_clear_text() {
    let diagnostics = Diagnostics::new("deepseek-harness");
    diagnostics.observe_native(
        &json!({"message":"rate limit", "willRetry":true}),
        "result",
        "turn",
    );
    assert_eq!(diagnostics.failure(), None);
    assert_eq!(diagnostics.message(), None);
    diagnostics.observe_native(
        &json!({"message":"rate limit", "willRetry":false}),
        "result",
        "turn",
    );
    assert!(
        diagnostics
            .message()
            .unwrap()
            .contains("retries did not recover")
    );
    diagnostics.restore(None);
    assert_eq!(diagnostics.message(), None);
}
