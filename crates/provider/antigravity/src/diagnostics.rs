use std::sync::{Arc, Mutex};

use tokio::io::AsyncRead;

/// Classified native failures; raw stderr never enters snapshots, logs or Debug output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Failure {
    Exit,
    Protocol,
    Timeout,
    Native,
    Authentication,
    Quota,
    Permission,
}

impl Failure {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Exit => {
                "AGY closed its output before the turn completed. Check the native CLI log."
            }
            Self::Protocol => {
                "AGY returned an invalid or inconsistent stream. Check the native CLI log."
            }
            Self::Timeout => "AGY did not respond before the operation timed out.",
            Self::Native => "AGY reported a model or API failure. Check the native CLI log.",
            Self::Authentication => {
                "AGY authentication failed. Run agy in a terminal to sign in again."
            }
            Self::Quota => {
                "AGY reported a quota or rate limit. Check your model quota before retrying."
            }
            Self::Permission => concat!(
                "AGY denied a tool requiring approval; headless mode cannot prompt for permission. ",
                "Add a scoped permissions.allow rule in AGY settings, or select Full Access in Ait.",
            ),
        }
    }

    pub(super) fn classify(text: &str) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        if text.contains("auto-denied")
            || text.contains("soft-denying")
            || text.contains("user denied permission")
            || text.contains("permission check failed")
        {
            Some(Self::Permission)
        } else if text.contains("quota") || text.contains("rate limit") {
            Some(Self::Quota)
        } else if text.contains("authentication required")
            || text.contains("not logged into antigravity")
            || text.contains("authentication failed")
        {
            Some(Self::Authentication)
        } else if text.contains("agy_error:") {
            Some(Self::Native)
        } else {
            None
        }
    }
}

/// Fixed protocol reasons contain no native payload, identifiers or filesystem paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum ProtocolError {
    #[error("AGY returned an event for a different conversation.")]
    Conversation,
    #[error("AGY returned an invalid step state.")]
    StepState,
    #[error("AGY returned a step without a valid type or index.")]
    StepIdentity,
    #[error("AGY updated a completed step or changed its type.")]
    StepLifecycle,
    #[error("AGY exceeded the stream size or step count limit.")]
    Limit,
    #[error("AGY returned invalid assistant text.")]
    Text,
    #[error("AGY returned invalid tool metadata.")]
    Tool,
    #[error("AGY returned an inconsistent checkpoint update.")]
    Checkpoint,
    #[error("AGY returned an invalid turn result.")]
    Result,
    #[error("AGY returned invalid token usage counters.")]
    Usage,
    #[error("AGY reported success before all steps completed.")]
    Unfinished,
    #[error("AGY returned invalid permission denial metadata.")]
    Denials,
    #[error("AGY returned an unexpected session event.")]
    Event,
}

impl ProtocolError {
    pub(super) fn source(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::StepState => "step_state",
            Self::StepIdentity => "step_identity",
            Self::StepLifecycle => "step_lifecycle",
            Self::Limit => "stream_limit",
            Self::Text => "assistant_text",
            Self::Tool => "tool_metadata",
            Self::Checkpoint => "checkpoint",
            Self::Result => "result",
            Self::Usage => "usage",
            Self::Unfinished => "unfinished_steps",
            Self::Denials => "denied_actions",
            Self::Event => "session_event",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Diagnostics(Arc<Mutex<Option<Failure>>>);

impl Diagnostics {
    pub(super) fn failure(&self) -> Option<Failure> {
        self.0.lock().ok().and_then(|failure| *failure)
    }

    pub(super) fn observe(&self, failure: Failure) {
        if let Ok(mut current) = self.0.lock()
            && current.is_none_or(|current| failure > current)
        {
            *current = Some(failure);
        }
    }

    pub(super) fn clear(&self) {
        if let Ok(mut current) = self.0.lock() {
            *current = None;
        }
    }

    pub(super) async fn drain(&self, output: impl AsyncRead + Unpin) {
        provider_native::diagnostics::drain_lines(output, |line| self.observe_line(line)).await;
    }

    fn observe_line(&self, line: &[u8]) {
        if let Some(failure) = Failure::classify(&String::from_utf8_lossy(line)) {
            self.observe(failure);
        }
    }
}

#[cfg(test)]
mod tests;
