//! Bounded native error explanations and classification; logs contain only structured facts.

pub(crate) mod failure;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::ports::diagnostics::{Category, Diagnostic};

const MAX_TEXT: usize = 32 * 1024;
const LOG_INTERVAL: Duration = Duration::from_secs(30);

/// Classify recognized native messages without retaining arbitrary text.
pub(crate) fn classify(text: &str) -> Category {
    let end = text.floor_char_boundary(text.len().min(MAX_TEXT));
    let text = text[..end].to_ascii_lowercase();
    if text.contains("insufficient_quota")
        || text.contains("quota exhausted")
        || text.contains("quota exceeded")
        || text.contains("insufficient credits")
        || text.contains("insufficient balance")
        || text.contains("credit balance")
        || text.contains("billing hard limit")
    {
        Category::Quota
    } else if text.contains("context_length_exceeded")
        || text.contains("context window")
        || text.contains("context limit")
        || text.contains("prompt is too long")
    {
        Category::ContextLimit
    } else if text.contains("model is at capacity")
        || text.contains("overloaded_error")
        || text.contains("model is overloaded")
    {
        Category::Capacity
    } else if text.contains("rate_limit")
        || text.contains("rate limit")
        || text.contains("too many requests")
    {
        Category::RateLimit
    } else if text.contains("authentication failed")
        || text.contains("authentication required")
        || text.contains("not logged in")
        || text.contains("not logged into")
        || text.contains("unauthorized")
        || text.contains("invalid api key")
        || text.contains("invalid_api_key")
        || text.contains("token expired")
        || text.contains("failed to authenticate")
        || text.contains("oauth session expired")
        || text.contains("authentication_error")
        || text.contains("invalid_grant")
        || text.contains("credentials expired")
    {
        Category::Authentication
    } else if text.contains("model_not_found")
        || text.contains("model not found")
        || text.contains("model does not exist")
        || text.contains("model is unavailable")
    {
        Category::Model
    } else if text.contains("permission denied")
        || text.contains("access denied")
        || text.contains("forbidden")
        || text.contains("can only be used from within opencode")
    {
        Category::Permission
    } else if text.contains("timed out") || text.contains("timeout") {
        Category::Timeout
    } else if text.contains("stream closed before response.completed")
        || text.contains("stream disconnected before completion")
    {
        Category::IncompleteResponse
    } else if text.contains("connection refused")
        || text.contains("connection reset")
        || text.contains("network error")
        || text.contains("network_error")
        || text.contains("failed to connect")
        || text.contains("stream disconnected")
    {
        Category::Network
    } else {
        Category::Unknown
    }
}

/// Read only recognized error facts, preferring structured classification over message text.
pub(crate) fn native(error: &Value, source: &'static str) -> Diagnostic {
    let info = error
        .get("codexErrorInfo")
        .or_else(|| error.pointer("/data/codexErrorInfo"));
    let class = info
        .and_then(Value::as_str)
        .or_else(|| {
            info.and_then(Value::as_object)
                .and_then(|value| value.keys().next().map(String::as_str))
        })
        .or_else(|| error["type"].as_str())
        .or_else(|| error["name"].as_str())
        .or_else(|| error["code"].as_str())
        .or_else(|| error.pointer("/error/type").and_then(Value::as_str))
        .or_else(|| error.pointer("/data/error/type").and_then(Value::as_str))
        .or_else(|| error.pointer("/data/errorName").and_then(Value::as_str));
    let category = match class {
        Some("contextWindowExceeded" | "ContextOverflowError" | "context_length_exceeded") => {
            Category::ContextLimit
        }
        Some("usageLimitExceeded" | "insufficient_quota") => Category::Quota,
        Some("httpConnectionFailed" | "responseStreamConnectionFailed") => Category::Network,
        Some("responseStreamDisconnected") => Category::IncompleteResponse,
        Some("authentication_error" | "authentication_failed" | "invalid_api_key") => {
            Category::Authentication
        }
        Some("permission_error") => Category::Permission,
        Some("rate_limit_error") => Category::RateLimit,
        Some("overloaded_error") => Category::Capacity,
        _ => Category::Unknown,
    };
    let status = error
        .get("statusCode")
        .or_else(|| error.get("status"))
        .or_else(|| error.get("httpStatusCode"))
        .or_else(|| error.pointer("/data/statusCode"))
        .or_else(|| error.pointer("/error/statusCode"))
        .or_else(|| {
            info.and_then(Value::as_object)
                .and_then(|info| info.values().find_map(|value| value.get("httpStatusCode")))
        })
        .and_then(Value::as_u64)
        .and_then(|status| u16::try_from(status).ok());
    let text = failure::native_message(error).unwrap_or_default();
    // Codex wraps upstream 4xx/5xx responses as connection failures. These broad
    // transport classes must not hide a recognized cause such as insufficient balance.
    let category = if matches!(category, Category::Unknown | Category::Network) {
        let structured = class.map_or(Category::Unknown, classify);
        let inferred = if structured == Category::Unknown {
            classify(&text)
        } else {
            structured
        };
        if inferred == Category::Unknown {
            category
        } else {
            inferred
        }
    } else {
        category
    };
    let category = if matches!(
        category,
        Category::Unknown | Category::Network | Category::IncompleteResponse
    ) {
        match status {
            Some(401) => Category::Authentication,
            Some(402) => Category::Quota,
            Some(403) => Category::Permission,
            Some(429) => Category::RateLimit,
            Some(408 | 504) => Category::Timeout,
            _ => category,
        }
    } else {
        category
    };
    Diagnostic {
        category,
        source,
        native_code: error["code"].as_i64(),
        http_status: status,
        will_retry: error["willRetry"].as_bool(),
    }
}

#[derive(Debug, Default)]
struct State {
    failure: Option<Diagnostic>,
    explanation: failure::Failure,
    logged: Option<(Diagnostic, Instant)>,
    suppressed: u64,
}

/// Shared process diagnostics retain sanitized explanations separately from structured logs.
#[derive(Debug, Clone)]
pub(crate) struct Diagnostics {
    provider: &'static str,
    state: Arc<Mutex<State>>,
}

impl Diagnostics {
    /// Create diagnostics for one owned native connection.
    pub(crate) fn new(provider: &'static str) -> Self {
        Self {
            provider,
            state: Arc::default(),
        }
    }

    /// Report safe OS failure facts without formatting paths or launch arguments.
    pub(crate) fn spawn_error(
        &self,
        error: &std::io::Error,
    ) -> crate::ports::agent_session::AgentSessionError {
        self.observe(Diagnostic::new(Category::Unavailable, "spawn"), "spawn");
        tracing::warn!(provider = self.provider, error_kind = ?error.kind(), os_error = error.raw_os_error(), "Native provider spawn failed");
        crate::ports::agent_session::AgentSessionError::Unavailable
    }

    /// Read the last non-retrying failure, with specific causes preferred over stream closure.
    pub(crate) fn failure(&self) -> Option<Diagnostic> {
        self.state.lock().ok().and_then(|state| state.failure)
    }

    /// Return a bounded, sanitized explanation copied from this connection's error fields.
    pub(crate) fn message(&self) -> Option<String> {
        let state = self.state.lock().ok()?;
        let message = state.explanation.message()?;
        let category = state.failure?.category;
        Some(
            if category == Category::Unknown || message.contains(category.message()) {
                message.to_owned()
            } else {
                format!("{message}\n{}", category.message())
            },
        )
    }

    /// Inspect an explicit native error, retaining its safe explanation without logging it.
    pub(crate) fn observe_native(&self, error: &Value, source: &'static str, operation: &str) {
        self.record(native(error, source), operation, true, Some(error));
    }

    /// Clear failure facts before a new turn; repeated log aggregation remains bounded.
    pub(crate) fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.failure = None;
            state.explanation.clear();
        }
    }

    /// Restore the failure fact captured before a request whose rejection was recovered.
    pub(crate) fn restore(&self, failure: Option<Diagnostic>) {
        if let Ok(mut state) = self.state.lock() {
            state.failure = failure;
            state.explanation.clear();
        }
    }

    /// Record a safe error and aggregate repeats rather than logging native messages.
    pub(crate) fn observe(&self, diagnostic: Diagnostic, operation: &str) {
        self.record(diagnostic, operation, true, None);
    }

    fn record(
        &self,
        diagnostic: Diagnostic,
        operation: &str,
        affects_turn: bool,
        error: Option<&Value>,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let provider = match self.provider {
            "claude" => "Claude Code",
            "opencode" => "OpenCode",
            "deepseek-harness" => "DeepSeek Harness",
            "pi" => "Pi",
            provider => provider,
        };
        if diagnostic.will_retry == Some(true)
            && let Some(error) = error
        {
            state.explanation.observe(provider, error, diagnostic);
        }
        if affects_turn
            && diagnostic.will_retry != Some(true)
            && (state
                .failure
                .is_none_or(|previous| previous.category == Category::Unknown)
                || (!matches!(
                    diagnostic.category,
                    Category::Unknown | Category::Closed | Category::Protocol
                ) && (error.is_some() || state.explanation.message().is_none())))
        {
            state.failure = Some(diagnostic);
            if let Some(error) = error {
                state.explanation.observe(provider, error, diagnostic);
            }
        }
        let now = Instant::now();
        if state.logged.is_some_and(|(previous, at)| {
            previous == diagnostic && now.duration_since(at) < LOG_INTERVAL
        }) {
            state.suppressed = state.suppressed.saturating_add(1);
            return;
        }
        if let Some((previous, _)) = state.logged.take()
            && state.suppressed > 0
        {
            self.log(previous, "repeated", state.suppressed);
        }
        state.suppressed = 0;
        state.logged = Some((diagnostic, now));
        self.log(diagnostic, operation, 0);
    }

    /// Flush aggregate counts on normal shutdown.
    pub(crate) fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some((previous, _)) = state.logged.take()
                && state.suppressed > 0
            {
                self.log(previous, "repeated", state.suppressed);
            }
            state.suppressed = 0;
        }
    }

    fn log(&self, diagnostic: Diagnostic, operation: &str, suppressed: u64) {
        tracing::warn!(
            provider = self.provider,
            source = diagnostic.source,
            operation,
            category = diagnostic.category.code(),
            native_code = diagnostic.native_code,
            http_status = diagnostic.http_status,
            will_retry = diagnostic.will_retry,
            suppressed,
            message = diagnostic.category.message(),
            "Native provider diagnostic"
        );
    }

    /// Drain stderr continuously with bounded line memory, retaining only recognized classes.
    pub(crate) async fn drain(self, mut output: impl AsyncRead + Unpin) {
        let mut chunk = [0; 4096];
        let mut line = Vec::with_capacity(4096);
        while let Ok(count @ 1..) = output.read(&mut chunk).await {
            for byte in &chunk[..count] {
                if *byte == b'\n' {
                    self.line(&line);
                    line.clear();
                } else if line.len() < MAX_TEXT {
                    line.push(*byte);
                }
            }
        }
        self.line(&line);
    }

    fn line(&self, line: &[u8]) {
        let text = String::from_utf8_lossy(line);
        let category = classify(&text);
        if category != Category::Unknown {
            // Model catalog refresh failures belong to discovery, rather than turn execution.
            let model_catalog = self.provider == "codex"
                && text.contains("codex_models_manager::manager")
                && text.contains("failed to refresh available models:");
            let (source, operation) = if model_catalog {
                ("model_catalog", "model/list")
            } else {
                ("stderr", "stderr")
            };
            self.record(
                Diagnostic::new(category, source),
                operation,
                !model_catalog,
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests;
