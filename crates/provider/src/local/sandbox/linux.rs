//! Linux write confinement through bubblewrap's mount and user namespaces.
//!
//! Reads and networking retain the host view. Writable bind mounts are followed by
//! read-only mounts for protected entries. No command is retried outside the sandbox.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use domain::workspace::sandbox::WritePolicy;
use tokio::process::Command;

use crate::ports::agent_session::AgentSessionError;

pub(crate) mod process;

const BWRAP: &str = "/usr/bin/bwrap";
const PROTECTED_DIRECTORIES: [&str; 4] = ["hooks", "plugins", "bin", "shims"];

/// Wrap `program` with writable mounts from `policy`, returning a command ready for args.
///
/// Missing state directories and empty protected configuration entries are initialized
/// before mounting, so absent protected paths cannot be created by the confined process.
/// Returns `Unavailable` without bubblewrap, or `Failed` if preparation fails.
pub(super) fn command(program: &OsStr, policy: &WritePolicy) -> Result<Command, AgentSessionError> {
    if !Path::new(BWRAP).is_file() {
        tracing::error!("Sandboxed worktrees require /usr/bin/bwrap (install bubblewrap)");
        return Err(AgentSessionError::Unavailable);
    }
    prepare(policy).map_err(|error| {
        tracing::error!(error_kind = ?error.kind(), "Linux sandbox mount preparation failed");
        AgentSessionError::Failed
    })?;
    let mut command = Command::new(BWRAP);
    command.args([
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--disable-userns",
        "--cap-drop",
        "ALL",
        "--ro-bind",
        "/",
        "/",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
    ]);
    let mut trees: Vec<&Path> = policy.trees().iter().map(PathBuf::as_path).collect();
    trees.extend([Path::new("/tmp"), Path::new("/var/tmp")]);
    // Pin writable ancestors too: otherwise renaming an ancestor could move its protected
    // mount away and let the Agent recreate an unprotected entry at the original name.
    for protected in policy.protected() {
        for parent in protected.ancestors().skip(1) {
            if trees.contains(&parent) || !trees.iter().any(|tree| parent.starts_with(tree)) {
                break;
            }
            trees.push(parent);
        }
    }
    trees.sort_unstable();
    trees.dedup();
    for path in trees {
        mount(&mut command, "--bind", path);
    }
    for path in policy.protected() {
        mount(&mut command, "--ro-bind", path);
    }
    command.arg("--").arg(program);
    Ok(command)
}

fn mount(command: &mut Command, option: &str, path: &Path) {
    command.arg(option).arg(path).arg(path);
}

fn prepare(policy: &WritePolicy) -> io::Result<()> {
    // Bind mounts cannot express filename prefixes. Claude relocates its global state
    // into its existing config directory, where the entire prefix is already writable.
    if policy
        .prefixes()
        .iter()
        .any(|prefix| !policy.trees().iter().any(|tree| prefix.starts_with(tree)))
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unmounted writable prefix",
        ));
    }
    for path in policy.trees() {
        fs::create_dir_all(path)?;
    }
    for path in policy.protected() {
        protect(path)?;
    }
    Ok(())
}

fn protect(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            // A mount follows symlinks; the writable link could then be replaced to bypass
            // the protected name. Refuse that layout instead of protecting only its target.
            return regular_entry(&metadata);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if path
        .file_name()
        .is_some_and(|name| PROTECTED_DIRECTORIES.iter().any(|entry| name == *entry))
    {
        return fs::create_dir_all(path);
    }
    // An absent commondir means the private Git directory is also the common one.
    // Materialize that same meaning so bind mounting the pointer does not break Git.
    let initial = if path.file_name().is_some_and(|name| name == "commondir") {
        b".\n".as_slice()
    } else if path
        .extension()
        .is_some_and(|extension| extension == "json" || extension == "jsonc" || extension == "yaml")
    {
        b"{}\n".as_slice()
    } else {
        b"".as_slice()
    };
    seed(path, initial)
}

fn regular_entry(metadata: &fs::Metadata) -> io::Result<()> {
    if (metadata.is_file() && metadata.nlink() == 1) || metadata.is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "protected entry must be a singly-linked regular file or directory",
        ))
    }
}

fn seed(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seed destination has no parent",
        )
    })?;
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    staging.write_all(contents)?;
    staging.as_file().sync_all()?;
    match staging.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
            regular_entry(&fs::symlink_metadata(path)?)
        }
        Err(error) => Err(error.error),
    }
}

/// Resolve Claude's sandbox config directory without granting writes to its home directory.
///
/// `variable` supplies the effective launch environment. An explicit config directory is
/// preserved. Otherwise seed the native global settings in `~/.claude/.claude.json` once;
/// credentials and history remain in the same `~/.claude` directory. Returns I/O failures.
pub(crate) fn claude_config_dir(variable: &dyn Fn(&str) -> Option<String>) -> io::Result<PathBuf> {
    if let Some(directory) = variable("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(directory));
    }
    let home = variable("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not absolute"))?;
    let directory = home.join(".claude");
    fs::create_dir_all(&directory)?;
    let destination = directory.join(".claude.json");
    if !destination.try_exists()? {
        match fs::read(home.join(".claude.json")) {
            Ok(contents) => seed(&destination, &contents)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(directory)
}

#[cfg(test)]
mod tests;
