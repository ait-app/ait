//! Bounded native turn/RPC explanations for the existing user-visible failure channel.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::ports::diagnostics::Diagnostic;

const MAX_NATIVE_BYTES: usize = 32 * 1024;
const MAX_REASON_BYTES: usize = 3072;

static URLS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)https?://[^\s<>"']+"#).expect("valid native error URL pattern")
});
static SECRETS: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
    [
        (
            Regex::new(r"(?im)\b(?:cookie|set-cookie)\s*:\s*[^\r\n]+")
                .expect("valid native error cookie pattern"),
            "Cookie: [redacted]",
        ),
        (
            Regex::new(concat!(
                r#"(?i)(["']?\b(?:authorization|proxy-authorization|api[_ -]?key|"#,
                r#"access[_ -]?token|refresh[_ -]?token|id[_ -]?token|token|"#,
                r#"client[_ -]?secret|secret|password|passwd|cookie|set-cookie)"#,
                r#"["']?\s*[:=]\s*)(?:"[^"\r\n]*"?|'[^'\r\n]*'?|"#,
                r"(?:bearer\s+)?[^\s,;\)\]\}>]+)",
            ))
            .expect("valid native error credential pattern"),
            "$1[redacted]",
        ),
        (
            Regex::new(r"(?i)\bbearer\s+[a-z0-9._~+/-]+=*")
                .expect("valid native error bearer pattern"),
            "Bearer [redacted]",
        ),
        (
            Regex::new(concat!(
                r"\b(?:(?:sk|pk|xox[baprs])-[A-Za-z0-9_-]{8,}|",
                r"gh[pousr]_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]{8,}|",
                r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)\b",
            ))
            .expect("valid native error token pattern"),
            "[redacted]",
        ),
    ]
});

/// User-facing failure state, separate from text-free diagnostic logs.
#[derive(Default)]
pub(crate) struct Failure {
    message: Option<String>,
    retried: bool,
    stopped_retrying: bool,
}

impl std::fmt::Debug for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Failure")
            .field("has_message", &self.message.is_some())
            .field("retried", &self.retried)
            .field("stopped_retrying", &self.stopped_retrying)
            .finish()
    }
}

impl Failure {
    /// Read the sanitized final explanation, without treating retrying errors as failures.
    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Transfer the explanation to its session before the native transport is discarded.
    pub(crate) fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// Forget the previous turn's explanation and native retry observations.
    pub(crate) fn clear(&mut self) {
        self.message = None;
        self.retried = false;
        self.stopped_retrying = false;
    }

    /// Retain only a bounded, sanitized native message and numeric failure facts.
    /// Retrying notifications record retry state without creating a final explanation.
    pub(crate) fn observe(&mut self, provider: &str, error: &Value, diagnostic: Diagnostic) {
        if diagnostic.will_retry == Some(true) {
            self.retried = true;
            return;
        }
        self.stopped_retrying |= diagnostic.will_retry == Some(false);
        let reason = native_message(error).and_then(|message| sanitize(&message));
        if reason.is_none() && self.message.is_some() {
            return;
        }
        let mut message = String::from(provider);
        if let Some(status) = diagnostic.http_status {
            let _ = write!(message, " (HTTP {status})");
        }
        if let Some(code) = diagnostic.native_code {
            let _ = write!(message, " (RPC {code})");
        }
        message.push_str(": ");
        message.push_str(reason.as_deref().unwrap_or(diagnostic.category.message()));
        if self.retried {
            let _ = write!(message, "\n{provider} retries did not recover the request.");
        } else if self.stopped_retrying {
            let _ = write!(message, "\n{provider} will not retry automatically.");
        }
        self.message = Some(message);
    }
}

pub(super) fn native_message(error: &Value) -> Option<Cow<'_, str>> {
    let message = error
        .as_str()
        .or_else(|| error.pointer("/data/error/message").and_then(Value::as_str))
        .or_else(|| error.pointer("/data/message").and_then(Value::as_str))
        .or_else(|| error.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| error["message"].as_str())
        .or_else(|| error["error"].as_str())?;
    // Some Codex failures carry a serialized API error in their message. Select its
    // explanation rather than exposing the surrounding response or extra diagnostics.
    if message.trim_start().starts_with(['{', '[']) {
        if message.len() > MAX_NATIVE_BYTES {
            return None;
        }
        let nested = serde_json::from_str::<Value>(message).ok()?;
        return nested
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| nested["message"].as_str())
            .map(|reason| Cow::Owned(reason.to_owned()));
    }
    Some(Cow::Borrowed(message))
}

fn sanitize(message: &str) -> Option<String> {
    let end = message.floor_char_boundary(message.len().min(MAX_NATIVE_BYTES));
    let origins = URLS.replace_all(&message[..end], |captures: &regex::Captures<'_>| {
        reqwest::Url::parse(&captures[0]).map_or_else(
            |_| "[redacted URL]".to_owned(),
            |url| url.origin().ascii_serialization(),
        )
    });
    let mut sanitized = origins.into_owned();
    for (pattern, replacement) in SECRETS.iter() {
        if let Cow::Owned(redacted) = pattern.replace_all(&sanitized, *replacement) {
            sanitized = redacted;
        }
    }
    sanitized.retain(|character| !character.is_control() || matches!(character, '\n' | '\t'));
    let clipped = end < message.len() || sanitized.len() > MAX_REASON_BYTES;
    sanitized.truncate(sanitized.floor_char_boundary(sanitized.len().min(MAX_REASON_BYTES)));
    let mut sanitized = sanitized.trim().to_owned();
    if sanitized.is_empty() {
        return None;
    }
    if clipped {
        sanitized.push_str(" [truncated]");
    }
    Some(sanitized)
}

#[cfg(test)]
mod tests;
