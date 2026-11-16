//! Apple Seatbelt backend: render a write-only SBPL profile and prepend `sandbox-exec`.
//!
//! The profile allows everything by default and denies only file writes, so dyld, Mach
//! services, TTYs and network keep working. Writable locations follow the macOS profile of
//! `@anthropic-ai/sandbox-runtime`; protected paths are denied after the allowances, and
//! Seatbelt applies the last matching rule.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::Path;

use domain::workspace::sandbox::WritePolicy;
use tokio::process::Command;

use provider_core::ports::agent_session::AgentSessionError;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// System temporary roots; `/tmp` and `/var` are symlinks to these real paths.
const SYSTEM_TREES: [&str; 2] = ["/private/tmp", "/private/var/folders"];

/// Devices that terminals, shells and runtimes open for writing.
const DEVICE_LITERALS: [&str; 8] = [
    "/dev/null",
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/dtracehelper",
    "/dev/ptmx",
    "/dev/stdout",
    "/dev/stderr",
];

/// Device families matched by name: terminals, pseudo-terminals and descriptor aliases.
const DEVICE_PATTERNS: [&str; 3] = ["^/dev/tty", "^/dev/pty", "^/dev/fd/"];

/// Prepend `sandbox-exec` with the rendered profile to `program`.
///
/// The profile is passed inline rather than as a file, so a confined Agent cannot rewrite
/// the profile that a later launch would load.
///
/// # Errors
///
/// Returns `Failed` when a policy path cannot be represented in SBPL.
pub(super) fn command(program: &OsStr, policy: &WritePolicy) -> Result<Command, AgentSessionError> {
    let profile = profile(policy).map_err(|error| {
        tracing::error!(%error, "Sandboxed worktree profile cannot be rendered");
        AgentSessionError::Failed
    })?;
    let mut command = Command::new(SANDBOX_EXEC);
    command.arg("-p").arg(profile).arg(program);
    Ok(command)
}

/// Policy path that SBPL cannot express safely.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum ProfileError {
    /// Seatbelt compares UTF-8 path strings.
    #[error("sandbox path is not valid UTF-8")]
    NotUtf8,
    /// Control characters could break out of an SBPL string literal.
    #[error("sandbox path contains a control character")]
    ControlCharacter,
}

/// Render the SBPL profile for `policy`.
///
/// # Errors
///
/// Returns `ProfileError` for a non-UTF-8 path or one containing control characters.
pub(super) fn profile(policy: &WritePolicy) -> Result<String, ProfileError> {
    let mut profile = String::with_capacity(1024);
    profile.push_str("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*");
    for tree in SYSTEM_TREES {
        rule(&mut profile, "subpath", tree);
    }
    for device in DEVICE_LITERALS {
        rule(&mut profile, "literal", device);
    }
    for pattern in DEVICE_PATTERNS {
        let _ = write!(profile, "\n    (regex #\"{pattern}\")");
    }
    for tree in policy.trees() {
        rule(&mut profile, "subpath", &escape(tree)?);
    }
    for prefix in policy.prefixes() {
        rule(&mut profile, "prefix", &escape(prefix)?);
    }
    profile.push_str(")\n");
    if !policy.protected().is_empty() {
        profile.push_str("(deny file-write*");
        for path in policy.protected() {
            rule(&mut profile, "subpath", &escape(path)?);
        }
        profile.push_str(")\n");
    }
    Ok(profile)
}

fn rule(profile: &mut String, filter: &str, path: &str) {
    let _ = write!(profile, "\n    ({filter} \"{path}\")");
}

/// Escape `path` as the body of an SBPL string literal.
fn escape(path: &Path) -> Result<String, ProfileError> {
    let path = path.to_str().ok_or(ProfileError::NotUtf8)?;
    if path.chars().any(char::is_control) {
        return Err(ProfileError::ControlCharacter);
    }
    let mut escaped = String::with_capacity(path.len());
    for character in path.chars() {
        if matches!(character, '"' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    Ok(escaped)
}

#[cfg(test)]
mod tests;
