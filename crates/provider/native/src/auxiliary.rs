//! Payload-free pre-send ownership markers for provider-owned auxiliary processes.
//!
//! A marker records an owned process group before inference is sent. Recovery only
//! confirms that the group is gone; it never signals a process whose ownership was
//! lost, resumes inference, or persists prompt text or credentials.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use provider_core::ports::agent_session::AgentSessionError;

#[derive(Debug, Serialize, Deserialize)]
struct Locator<'a> {
    request_id: &'a str,
    topic_id: &'a str,
    thread_id: &'a str,
    process_group: u32,
}

/// Derive a traversal-safe ownership-marker path from the manager's directory and request key.
#[must_use]
pub fn path(directory: &Path, request_id: &str) -> PathBuf {
    directory.join(format!(
        "request-{:x}.json",
        Sha256::digest(request_id.as_bytes())
    ))
}

/// Persist the native locator before inference; an existing key must never execute again.
///
/// # Errors
/// Returns a failure if a marker already exists, cannot be written or cannot be synchronized.
pub fn create(
    path: &Path,
    request_id: &str,
    topic_id: &str,
    thread_id: &str,
    process_group: u32,
) -> Result<(), AgentSessionError> {
    let directory = path.parent().ok_or(AgentSessionError::Failed)?;
    std::fs::create_dir_all(directory).map_err(|_| AgentSessionError::Unavailable)?;
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| AgentSessionError::Unavailable)?;
    serde_json::to_writer(
        &mut file,
        &Locator {
            request_id,
            topic_id,
            thread_id,
            process_group,
        },
    )
    .map_err(|_| AgentSessionError::Unavailable)?;
    file.flush()
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| AgentSessionError::Unavailable)?;
    file.persist_noclobber(path).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            AgentSessionError::Failed
        } else {
            AgentSessionError::Unavailable
        }
    })?;
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| AgentSessionError::Failed)
}

/// Remove a settled marker; missing files are harmless.
///
/// # Errors
/// Returns a failure when the filesystem refuses removal.
pub fn remove(path: &Path) -> Result<(), AgentSessionError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AgentSessionError::Failed),
    }
}

/// Confirm an old execution is gone without signalling a PID whose ownership was lost.
/// A live or inaccessible group stays uncertain; it is never killed or re-executed.
///
/// # Errors
/// Returns a failure for an unreadable, corrupt or mismatched marker, or a live/unknown group.
pub fn reconcile(path: &Path, request_id: &str) -> Result<(), AgentSessionError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(AgentSessionError::Failed),
    };
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() > 16 * 1024 {
        return Err(AgentSessionError::Failed);
    }
    let locator: Locator<'_> =
        serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?;
    if locator.request_id != request_id || !group_gone(locator.process_group)? {
        return Err(AgentSessionError::Failed);
    }
    remove(path)
}

fn group_gone(group: u32) -> Result<bool, AgentSessionError> {
    #[cfg(unix)]
    {
        let group = i32::try_from(group)
            .ok()
            .filter(|group| *group > 0)
            .ok_or(AgentSessionError::Failed)?;
        match nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), None) {
            Err(nix::errno::Errno::ESRCH) => Ok(true),
            Ok(()) => Ok(false),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = group;
        Err(AgentSessionError::Unavailable)
    }
}

#[cfg(all(test, unix))]
mod tests;
