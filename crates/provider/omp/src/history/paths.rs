//! Where OMP keeps session files and image blobs (D20), resolved as OMP 18.18.8 and Paseo's
//! `session-descriptor.ts` resolve them, and the bounded scan for session files.

use std::path::{Path, PathBuf};

use super::super::OmpClient;
use provider_core::ports::agent_session::AgentSessionError;

/// Directory entries inspected per scan; later entries are ignored rather than failing.
const MAX_ENTRIES: usize = 32_768;
/// OMP's configuration directory under the home directory, unless `PI_CONFIG_DIR` names one.
const CONFIG_DIR: &str = ".omp";
/// Valid profile names, as OMP's `dirs.ts` and Paseo's `resolveOmpProfile` define them.
const PROFILE_BYTES: usize = 64;

/// One variable from the client's private environment, else from the daemon's.
fn environment(client: &OmpClient, key: &str) -> Option<String> {
    client
        .environment
        .entries()
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
        .or_else(|| std::env::var(key).ok())
}

/// OMP's agent directory and the profile it belongs to.
struct Agent {
    directory: PathBuf,
    profile: Option<String>,
    /// The profile's configuration root: `~/.omp` (or `PI_CONFIG_DIR`), plus
    /// `profiles/<profile>` for a named profile.
    config: PathBuf,
    /// `PI_CODING_AGENT_DIR` replaced the default directory, so XDG roots are not consulted.
    overridden: bool,
}

/// OMP's roots for one profile, as Paseo's diagnostic reports them (D31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Roots {
    /// Selected profile, or `None` for the default one.
    pub(crate) profile: Option<String>,
    /// The profile's configuration root.
    pub(crate) config: PathBuf,
    /// The agent directory.
    pub(crate) agent: PathBuf,
    /// Existing XDG data, state and cache roots; each is consulted only with the default agent
    /// directory on Linux and macOS.
    pub(crate) xdg: [Option<PathBuf>; 3],
}

impl Roots {
    /// OMP's agent database: in the XDG data root when there is one, else the agent directory.
    pub(crate) fn database(&self) -> PathBuf {
        self.xdg[0]
            .as_deref()
            .unwrap_or(&self.agent)
            .join("agent.db")
    }
}

/// The selected profile: `OMP_PROFILE` when set, even empty, else `PI_PROFILE`; blank or
/// `default` select none. Returns rejected for a name OMP does not accept: OMP 18.18.8 exits
/// with `Invalid OMP profile` rather than falling back to the default profile.
fn profile(client: &OmpClient) -> Result<Option<String>, AgentSessionError> {
    let Some(value) =
        environment(client, "OMP_PROFILE").or_else(|| environment(client, "PI_PROFILE"))
    else {
        return Ok(None);
    };
    let profile = value.trim();
    if profile.is_empty() || profile == "default" {
        return Ok(None);
    }
    if !valid_profile(profile) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(Some(profile.to_owned()))
}

/// OMP's profile rule: `[a-z0-9][a-z0-9._-]{0,63}`, not ending in `.`, and not a Windows
/// reserved device name with or without an extension.
fn valid_profile(profile: &str) -> bool {
    let bytes = profile.as_bytes();
    let first = bytes.first().copied().unwrap_or_default();
    if bytes.len() > PROFILE_BYTES
        || !(first.is_ascii_lowercase() || first.is_ascii_digit())
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        || profile.ends_with('.')
    {
        return false;
    }
    let device = profile.split('.').next().unwrap_or_default();
    let reserved = ["con", "prn", "aux", "nul"].contains(&device)
        || ["com", "lpt"].iter().any(|prefix| {
            device
                .strip_prefix(prefix)
                .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
        });
    !reserved
}

/// Resolve a configured path as Paseo's `resolveConfigPath`: `~` and `~/` name the home
/// directory, and a relative path is relative to `base`.
fn configured(value: &str, home: &Path, base: Option<&Path>) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value == "~" {
        return Some(home.to_path_buf());
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return Some(home.join(relative));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Some(path);
    }
    base.map(|base| base.join(path))
}

fn home(client: &OmpClient) -> Result<PathBuf, AgentSessionError> {
    environment(client, "HOME")
        .or_else(|| environment(client, "USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or(AgentSessionError::Unavailable)
}

/// The directory a relative environment path is resolved against: the session's working
/// directory, where OMP itself runs, else the daemon's.
fn base(cwd: Option<&str>) -> Option<PathBuf> {
    cwd.map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
}

fn agent(client: &OmpClient, cwd: Option<&str>) -> Result<Agent, AgentSessionError> {
    let home = home(client)?;
    let profile = profile(client)?;
    let mut config = home.join(
        environment(client, "PI_CONFIG_DIR")
            .filter(|directory| !directory.is_empty())
            .unwrap_or_else(|| CONFIG_DIR.to_owned()),
    );
    if let Some(profile) = &profile {
        config = config.join("profiles").join(profile);
    }
    if profile.is_none()
        && let Some(directory) = environment(client, "PI_CODING_AGENT_DIR")
            .and_then(|directory| configured(&directory, &home, base(cwd).as_deref()))
    {
        return Ok(Agent {
            directory,
            profile,
            config,
            overridden: true,
        });
    }
    Ok(Agent {
        directory: config.join("agent"),
        profile,
        config,
        overridden: false,
    })
}

/// An existing XDG `omp[/profiles/<profile>]` directory under `variable`'s base, consulted
/// only when the agent directory is the default one.
fn xdg(client: &OmpClient, agent: &Agent, variable: &str) -> Option<PathBuf> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if !agent.overridden
        && let Some(base) = environment(client, variable).filter(|base| !base.is_empty())
    {
        let mut root = PathBuf::from(base).join("omp");
        if let Some(profile) = &agent.profile {
            root = root.join("profiles").join(profile);
        }
        return root.is_dir().then_some(root);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = (client, agent, variable);
    None
}

/// OMP's data root: an existing XDG data root, else the agent directory.
fn data(client: &OmpClient, agent: &Agent) -> PathBuf {
    xdg(client, agent, "XDG_DATA_HOME").unwrap_or_else(|| agent.directory.clone())
}

/// OMP's roots for the daemon's environment and the client's overrides; a relative agent
/// directory resolves against the daemon's working directory, as Paseo resolves it.
///
/// # Errors
///
/// Returns unavailable without a home directory, or rejected for an invalid profile.
pub(crate) fn roots(client: &OmpClient) -> Result<Roots, AgentSessionError> {
    let agent = agent(client, None)?;
    let xdg = ["XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"]
        .map(|variable| xdg(client, &agent, variable));
    Ok(Roots {
        profile: agent.profile,
        config: agent.config,
        agent: agent.directory,
        xdg,
    })
}

/// The directory OMP writes session files to for `cwd`: `PI_CODING_AGENT_SESSION_DIR`, else
/// the data root's `sessions`.
///
/// OMP 18.18.8 honours only the environment variable (and `--session-dir`), writing session
/// files directly into it. Unlike Paseo, a `sessionDir` in `<cwd>/.omp/settings.json` or
/// `<agentDir>/settings.json` is not read: OMP migrates the agent's `settings.json` into
/// `config.yml` and ignores `sessionDir` in either, so it never writes there.
///
/// # Errors
///
/// Returns unavailable without a home directory, or rejected for an invalid profile.
pub(crate) fn sessions(
    client: &OmpClient,
    cwd: Option<&str>,
) -> Result<PathBuf, AgentSessionError> {
    let home = home(client)?;
    let base = base(cwd);
    if let Some(directory) = environment(client, "PI_CODING_AGENT_SESSION_DIR")
        .and_then(|directory| configured(&directory, &home, base.as_deref()))
    {
        return Ok(directory);
    }
    let agent = agent(client, cwd)?;
    Ok(data(client, &agent).join("sessions"))
}

/// OMP's content-addressed image store: always the data root's `blobs`, whatever directory
/// holds the session files (verified with OMP 18.18.8 under `PI_CODING_AGENT_SESSION_DIR`).
///
/// # Errors
///
/// Returns unavailable without a home directory, or rejected for an invalid profile.
pub(crate) fn blobs(client: &OmpClient, cwd: Option<&str>) -> Result<PathBuf, AgentSessionError> {
    let agent = agent(client, cwd)?;
    Ok(data(client, &agent).join("blobs"))
}

/// Collect session files under `root` newest first: files in `root` (a session-directory
/// override) and in its per-workspace subdirectories (the default layout). A directory named
/// after a sibling session file holds that session's artifacts and subagent transcripts, so
/// it is not a workspace.
///
/// # Errors
///
/// Returns failed when a directory cannot be read for a reason other than its absence.
pub(super) fn scan(root: &Path) -> Result<Vec<PathBuf>, AgentSessionError> {
    scan_within(root, MAX_ENTRIES)
}

/// [`scan`], inspecting at most `limit` directory entries.
fn scan_within(root: &Path, limit: usize) -> Result<Vec<PathBuf>, AgentSessionError> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    let mut scanned = 0;
    while let Some(directory) = directories.pop() {
        if scanned >= limit {
            break;
        }
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        for entry in entries.take(limit - scanned) {
            scanned += 1;
            let Some(entry) = present(entry)? else {
                continue;
            };
            let Some(kind) = present(entry.file_type())? else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir()
                && path.parent() == Some(root)
                && !path.with_extension("jsonl").is_file()
            {
                directories.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            {
                files.push(path);
            }
        }
    }
    files.sort_by_cached_key(|path| {
        std::cmp::Reverse(
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok(),
        )
    });
    Ok(files)
}

/// Treat an entry removed by a native writer after enumeration as absent.
fn present<T>(result: std::io::Result<T>) -> Result<Option<T>, AgentSessionError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(AgentSessionError::Failed),
    }
}

#[cfg(test)]
mod tests;
