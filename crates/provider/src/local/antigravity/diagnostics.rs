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
