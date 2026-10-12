//! Bounded native error classification and logging; raw messages are only inspected in memory.

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
        .or_else(|| error.pointer("/data/errorName").and_then(Value::as_str));
    let category = match class {
        Some("contextWindowExceeded" | "ContextOverflowError" | "context_length_exceeded") => {
            Category::ContextLimit
        }
        Some("usageLimitExceeded" | "insufficient_quota") => Category::Quota,
        Some(
            "httpConnectionFailed"
            | "responseStreamConnectionFailed"
            | "responseStreamDisconnected",
        ) => Category::Network,
        Some("authentication_error") => Category::Authentication,
        Some("rate_limit_error") => Category::RateLimit,
        Some("overloaded_error") => Category::Capacity,
        _ => Category::Unknown,
    };
    let status = error
        .get("statusCode")
        .or_else(|| error.get("httpStatusCode"))
        .or_else(|| error.pointer("/data/statusCode"))
        .or_else(|| {
            info.and_then(Value::as_object)
                .and_then(|info| info.values().find_map(|value| value.get("httpStatusCode")))
        })
        .and_then(Value::as_u64)
        .and_then(|status| u16::try_from(status).ok());
    let text = error
        .as_str()
        .or_else(|| error["message"].as_str())
        .or_else(|| error.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| error["error"].as_str())
        .unwrap_or_default();
    let category = if category == Category::Unknown {
        let structured = class.map_or(Category::Unknown, classify);
        if structured == Category::Unknown {
            classify(text)
        } else {
            structured
        }
    } else {
        category
    };
    let category = if category == Category::Unknown {
        match status {
            Some(401) => Category::Authentication,
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
        will_retry: None,
    }
}

#[derive(Debug, Default)]
struct State {
    failure: Option<Diagnostic>,
    logged: Option<(Diagnostic, Instant)>,
    suppressed: u64,
}

/// Shared process diagnostics survive reader tasks and never store native text.
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

    /// Clear failure facts before a new turn; repeated log aggregation remains bounded.
    pub(crate) fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.failure = None;
        }
    }

    /// Record a safe error and aggregate repeats rather than logging native messages.
    pub(crate) fn observe(&self, diagnostic: Diagnostic, operation: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if diagnostic.will_retry != Some(true)
            && (state
                .failure
                .is_none_or(|previous| previous.category == Category::Unknown)
                || !matches!(
                    diagnostic.category,
                    Category::Unknown | Category::Closed | Category::Protocol
                ))
        {
            state.failure = Some(diagnostic);
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
        let category = classify(&String::from_utf8_lossy(line));
        if category != Category::Unknown {
            self.observe(Diagnostic::new(category, "stderr"), "stderr");
        }
    }
}

#[cfg(test)]
mod tests;
