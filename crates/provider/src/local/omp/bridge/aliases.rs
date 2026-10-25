//! Ait-owned ACP aliases for empty native sessions replaced by CLI-bound settings.

use std::io::{Read, Write};
use std::path::PathBuf;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde::{Deserialize, Serialize};

use crate::local::omp::{OmpClient, history};
use crate::ports::agent_session::AgentSessionError;

const MAX_BYTES: u64 = 1024 * 1024;

/// Workspace-bound ACP identity and the current official native resume facts.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Alias {
    pub(super) cwd: String,
    pub(super) handle: AgentPersistenceHandle,
    pub(super) config: StoredAgentConfig,
}

fn path(client: &OmpClient, id: &str) -> Result<PathBuf, AgentSessionError> {
    uuid::Uuid::parse_str(id).map_err(|_| AgentSessionError::Rejected)?;
    let root = history::root(client)?;
    Ok(root
        .with_file_name("ait-acp-aliases")
        .join(format!("{id}.json")))
}

/// Resolve an ACP `id` for the same `cwd`; missing aliases fall back to native lookup.
/// Rejects malformed, oversized, cross-workspace or symlink records before opening OMP.
pub(super) fn load(
    client: &OmpClient,
    id: &str,
    cwd: &str,
) -> Result<Option<Alias>, AgentSessionError> {
    let path = path(client, id)?;
    if let Some(directory) = path.parent() {
        match std::fs::symlink_metadata(directory) {
            Ok(metadata) if !metadata.file_type().is_dir() => {
                return Err(AgentSessionError::Rejected);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(AgentSessionError::Failed),
            Ok(_) => {}
        }
    }
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AgentSessionError::Failed),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(AgentSessionError::Rejected);
    }
    let file = std::fs::File::open(path).map_err(|_| AgentSessionError::Failed)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(AgentSessionError::Rejected);
    }
    let alias: Alias = serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?;
    if alias.cwd != cwd || alias.handle.provider != "omp" {
        return Err(AgentSessionError::Rejected);
    }
    Ok(Some(alias))
}

/// Atomically retain an ACP `id` and native handle without modifying OMP JSONL or settings.
/// Returns bounded serialization, directory or atomic write failures.
pub(super) fn save(client: &OmpClient, id: &str, alias: &Alias) -> Result<(), AgentSessionError> {
    let path = path(client, id)?;
    let directory = path.parent().ok_or(AgentSessionError::Failed)?;
    let bytes = serde_json::to_vec(alias).map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(AgentSessionError::Rejected);
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|_| AgentSessionError::Failed)?;
    if !std::fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(AgentSessionError::Rejected);
    }
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| AgentSessionError::Failed)?;
    file.write_all(&bytes)
        .map_err(|_| AgentSessionError::Failed)?;
    file.persist(path).map_err(|_| AgentSessionError::Failed)?;
    Ok(())
}
