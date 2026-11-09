//! OS write confinement for native provider processes launched in a sandboxed Workspace.
//!
//! [`command`] is the single platform seam: it resolves the domain [`WritePolicy`] for one
//! launch and hands it to the platform backend. macOS uses Seatbelt and Linux uses
//! bubblewrap; other platforms fail closed.

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use domain::workspace::sandbox::{WriteConfinement, WritePolicy};
use tokio::process::Command;

use crate::ports::agent_session::AgentSessionError;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
mod seatbelt;

/// How a provider state location is matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateMatch {
    /// The directory and everything below it.
    Tree,
    /// Every path spelled with this prefix, such as a state file and its atomic-write siblings.
    Prefix,
}

/// One provider-owned state location that a confined Agent process must keep writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateLocation {
    /// Environment variable naming the location, or its base directory, when set.
    variable: Option<&'static str>,
    /// Child below the variable's value; empty when the variable names the location itself.
    child: &'static str,
    /// Location below the home directory when the variable is unset.
    home: &'static str,
    /// Directory tree or path-spelling prefix.
    matching: StateMatch,
    /// Entries below the location that unconfined sessions execute or obey.
    protected: &'static [&'static str],
}

impl StateLocation {
    const fn tree(
        variable: Option<&'static str>,
        child: &'static str,
        home: &'static str,
        protected: &'static [&'static str],
    ) -> Self {
        Self {
            variable,
            child,
            home,
            matching: StateMatch::Tree,
            protected,
        }
    }
}

/// Claude Code configuration, credentials index, session history and MCP logs.
pub(crate) const CLAUDE_STATE: &[StateLocation] = &[
    StateLocation::tree(
        Some("CLAUDE_CONFIG_DIR"),
        "",
        ".claude",
        &["settings.json", "plugins"],
    ),
    StateLocation {
        variable: Some("CLAUDE_CONFIG_DIR"),
        child: ".claude.json",
        home: ".claude.json",
        matching: StateMatch::Prefix,
        protected: &[],
    },
    StateLocation::tree(None, "", ".cache/claude", &[]),
    #[cfg(target_os = "macos")]
    StateLocation::tree(None, "", "Library/Caches/claude-cli-nodejs", &[]),
];

/// Codex sessions, logs, authentication and state databases.
pub(crate) const CODEX_STATE: &[StateLocation] = &[StateLocation::tree(
    Some("CODEX_HOME"),
    "",
    ".codex",
    &["config.toml"],
)];

/// Pi agent configuration and session history.
pub(crate) const PI_STATE: &[StateLocation] = &[
    StateLocation::tree(
        Some("PI_CODING_AGENT_DIR"),
        "",
        ".pi/agent",
        &["settings.json"],
    ),
    StateLocation::tree(
        Some("PI_CODING_AGENT_SESSION_DIR"),
        "",
        ".pi/agent/sessions",
        &[],
    ),
];

/// Oh My Pi configuration and data directories.
pub(crate) const OMP_STATE: &[StateLocation] = &[
    StateLocation::tree(Some("PI_CONFIG_DIR"), "", ".omp", &[]),
    StateLocation::tree(Some("XDG_DATA_HOME"), "omp", ".local/share/omp", &[]),
];

/// `OpenCode` XDG data, state, cache and configuration directories.
pub(crate) const OPENCODE_STATE: &[StateLocation] = &[
    StateLocation::tree(
        Some("XDG_DATA_HOME"),
        "opencode",
        ".local/share/opencode",
        &[],
    ),
    StateLocation::tree(
        Some("XDG_STATE_HOME"),
        "opencode",
        ".local/state/opencode",
        &[],
    ),
    StateLocation::tree(Some("XDG_CACHE_HOME"), "opencode", ".cache/opencode", &[]),
    StateLocation::tree(
        Some("XDG_CONFIG_HOME"),
        "opencode",
        ".config/opencode",
        &["opencode.json", "opencode.jsonc", "config.json"],
    ),
];

/// `MiniMax` Code configuration, sessions and runtime state.
pub(crate) const MINIMAX_STATE: &[StateLocation] = &[StateLocation::tree(
    None,
    "",
    ".minimax",
    &["config.yaml", "bin", "plugins", "shims"],
)];

/// Build the native command for `program`, confined when `confinement` is present.
///
/// # Arguments
///
/// * `program` - Native executable, exactly as the adapter would launch it unconfined.
/// * `confinement` - Sandboxed Workspace placement, or none for an ordinary launch.
/// * `state` - Provider state locations the native process must keep writing.
/// * `variable` - Environment lookup that sees the adapter's overrides before the daemon's.
///
/// # Returns
///
/// A command whose later arguments, environment and working directory reach `program`.
///
/// # Errors
///
/// Returns `Unavailable` when this platform has no write sandbox, and `Failed` when the
/// worktree, its Git directories or the home directory cannot be resolved.
pub(crate) fn command(
    program: &Path,
    confinement: Option<&WriteConfinement>,
    state: &[StateLocation],
    variable: &dyn Fn(&str) -> Option<String>,
) -> Result<Command, AgentSessionError> {
    let Some(confinement) = confinement else {
        return Ok(Command::new(program));
    };
    confined(program, confinement, state, variable)
}

fn confined(
    program: &Path,
    confinement: &WriteConfinement,
    state: &[StateLocation],
    variable: &dyn Fn(&str) -> Option<String>,
) -> Result<Command, AgentSessionError> {
    let policy = policy(confinement, state, variable).map_err(|error| {
        tracing::error!(
            error_kind = ?error.kind(),
            "Sandboxed worktree write policy is unresolvable"
        );
        AgentSessionError::Failed
    })?;
    backend(program.as_os_str(), &policy)
}

#[cfg(target_os = "macos")]
use seatbelt::command as backend;

#[cfg(target_os = "linux")]
use linux::command as backend;

/// Refuse to launch unconfined where no write sandbox backend exists yet.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn backend(_program: &OsStr, _policy: &WritePolicy) -> Result<Command, AgentSessionError> {
    tracing::error!(
        "Sandboxed worktree Agents require macOS or Linux; refusing an unconfined launch"
    );
    Err(AgentSessionError::Unavailable)
}

/// Resolve the writable locations for one confined launch.
///
/// # Errors
///
/// Returns resolution failures for the worktree, its Git directories or the home directory.
fn policy(
    confinement: &WriteConfinement,
    state: &[StateLocation],
    variable: &dyn Fn(&str) -> Option<String>,
) -> io::Result<WritePolicy> {
    let worktree = confinement.worktree_root().canonicalize()?;
    let (git_dir, common_dir) = git_dirs(&worktree)?;
    let mut policy = WritePolicy::worktree(&worktree, &git_dir, &common_dir);
    let home = variable("HOME")
        .filter(|home| Path::new(home).is_absolute())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not absolute"))?;
    for location in state {
        let configured = location
            .variable
            .and_then(variable)
            .filter(|value| !value.is_empty());
        let path = configured.map_or_else(
            || home.join(location.home),
            |value| home.join(value).join(location.child),
        );
        let path = real_path(&path)?;
        for entry in location.protected {
            policy.protect(path.join(entry));
        }
        match location.matching {
            StateMatch::Tree => policy.allow_tree(path),
            StateMatch::Prefix => policy.allow_prefix(path),
        }
    }
    if let Some(temporary) = variable("TMPDIR").filter(|path| Path::new(path).is_absolute()) {
        policy.allow_tree(real_path(Path::new(&temporary))?);
    }
    Ok(policy)
}

/// Return the private and common Git directories of the checkout at real path `worktree`.
fn git_dirs(worktree: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let marker = worktree.join(".git");
    if marker.is_dir() {
        let git_dir = marker.canonicalize()?;
        return Ok((git_dir.clone(), git_dir));
    }
    let contents = std::fs::read_to_string(&marker)?;
    let pointer = contents
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|pointer| !pointer.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed .git file"))?;
    let git_dir = worktree.join(pointer).canonicalize()?;
    let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(common) => git_dir.join(common.trim()).canonicalize()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => git_dir.clone(),
        Err(error) => return Err(error),
    };
    Ok((git_dir, common_dir))
}

/// Resolve `path` through its nearest existing ancestor, so a missing state directory still
/// matches the real location Seatbelt compares against once the provider creates it.
fn real_path(path: &Path) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "state paths must be normalized",
        ));
    }
    let mut missing: Vec<&OsStr> = Vec::new();
    let mut existing = path;
    loop {
        match existing.canonicalize() {
            Ok(real) => {
                return Ok(missing
                    .iter()
                    .rev()
                    .fold(real, |path, name| path.join(name)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or(error)?);
                existing = existing
                    .parent()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
