//! Advisory release information; individual ACP capabilities decide feature support.

use std::{ffi::OsStr, path::Path};

use crate::{AntigravityClient, transport};

/// Minimum AGY release whose ACP server settles background work and cancelled tools cleanly.
pub(crate) const COMPLETION_FIX_VERSION: (u64, u64, u64) = (1, 5, 15);

/// Release information used only for diagnostics and feature-local completion notices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    /// Installed CLI includes the completion fixes in [`COMPLETION_FIX_VERSION`].
    Supported(String),
    /// Installed CLI can still use ACP but predates the completion fixes.
    Legacy(String),
    /// Version output could not be queried or parsed as a stable release.
    Unverified,
}

impl Status {
    /// Whether this release is known to include the background-work and cancellation fixes.
    pub(crate) const fn has_completion_fixes(&self) -> bool {
        matches!(self, Self::Supported(_))
    }

    /// Advisory guidance that never disables otherwise supported session operations.
    pub(crate) fn upgrade_guidance(&self) -> Option<String> {
        let (major, minor, patch) = COMPLETION_FIX_VERSION;
        match self {
            Self::Supported(_) => None,
            Self::Legacy(version) => Some(format!(
                "Antigravity CLI (agy) {version} remains usable through ACP. \
                 Upgrade agy to {major}.{minor}.{patch} or newer for reliable background-task, subagent and cancellation completion."
            )),
            Self::Unverified => Some(format!(
                "Antigravity CLI (agy) version could not be verified. \
                 Available ACP features remain usable; upgrade agy to {major}.{minor}.{patch} or newer for background-task and cancellation fixes."
            )),
        }
    }
}

/// Read `agy --version` for diagnostics only; an unknown version does not block ACP.
pub(crate) async fn probe(client: &AntigravityClient, cwd: &Path) -> Status {
    let Ok(bytes) = transport::query(client, cwd, &[OsStr::new("--version")]).await else {
        return Status::Unverified;
    };
    classify(&bytes)
}

/// Classify a CLI version without treating an unrecognized release as unsupported.
pub(super) fn classify(bytes: &[u8]) -> Status {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Status::Unverified;
    };
    let trimmed = text.trim();
    let Some(parsed) = parse(trimmed) else {
        return Status::Unverified;
    };
    if parsed >= COMPLETION_FIX_VERSION {
        Status::Supported(trimmed.to_owned())
    } else {
        Status::Legacy(trimmed.to_owned())
    }
}

fn parse(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests;
