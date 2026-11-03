//! Native ZIP delivery and repeatable decoration of the original command journal item.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::http::Api;
use crate::{ports::agent_session::AgentSessionError, protocol::timeline::NativeItem};

const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

/// Private exported files keyed by stable native command identity, outside the project tree.
#[derive(Debug, Clone)]
pub(in crate::local::deepseek_harness) struct Store {
    directory: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join(format!("ait-dsh-exports-{}", std::process::id())))
    }
}

impl Store {
    /// Use `directory` for private artifacts shared by live events and native history replay.
    pub(in crate::local::deepseek_harness) const fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn directory(&self, identity: &str) -> PathBuf {
        self.directory
            .join(format!("{:x}", Sha256::digest(identity.as_bytes())))
    }

    /// Download this native session tree after its first-party export command succeeds.
    /// Rejects HTTP, malformed/truncated ZIPs and the 1 GiB budget; partial files stay private
    /// and are removed on cancellation or failure. No browser cookies enter the artifact.
    pub(super) async fn download(
        &self,
        api: &Api,
        session: &str,
        command: &str,
    ) -> Result<(), AgentSessionError> {
        let response = api.export_session(session).await?;
        if response
            .content_length()
            .is_some_and(|bytes| bytes > MAX_ARCHIVE_BYTES)
        {
            return Err(AgentSessionError::Failed);
        }
        let directory = self.directory(&format!("dsh:{session}:command:{command}"));
        let root = self.directory.clone();
        let temporary = tokio::task::spawn_blocking(move || {
            private_directory(&root)?;
            private_directory(&directory)?;
            tempfile::NamedTempFile::new_in(directory).map_err(|_| AgentSessionError::Failed)
        })
        .await
        .map_err(|_| AgentSessionError::Failed)??;
        let file = temporary
            .as_file()
            .try_clone()
            .map_err(|_| AgentSessionError::Failed)?;
        let mut file = tokio::fs::File::from_std(file);
        let mut chunks = response.bytes_stream();
        let mut bytes = 0_u64;
        let mut header = Vec::with_capacity(4);
        let mut tail = Vec::with_capacity(22);
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| AgentSessionError::Failed)?;
            bytes = bytes
                .checked_add(chunk.len() as u64)
                .filter(|size| *size <= MAX_ARCHIVE_BYTES)
                .ok_or(AgentSessionError::Failed)?;
            header.extend_from_slice(&chunk[..(4 - header.len()).min(chunk.len())]);
            if chunk.len() >= 22 {
                tail.clear();
                tail.extend_from_slice(&chunk[chunk.len() - 22..]);
            } else {
                let discard = tail.len().saturating_add(chunk.len()).saturating_sub(22);
                tail.drain(..discard);
                tail.extend_from_slice(&chunk);
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| AgentSessionError::Failed)?;
        }
        // The native fflate writer emits a nonempty, single-disk ZIP without an archive comment.
        if header != b"PK\x03\x04"
            || tail.len() != 22
            || &tail[..4] != b"PK\x05\x06"
            || tail[20..] != [0, 0]
            || bytes < 52
        {
            return Err(AgentSessionError::Failed);
        }
        file.sync_all()
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        drop(file);
        tokio::task::spawn_blocking(move || {
            let target = temporary
                .path()
                .parent()
                .ok_or(AgentSessionError::Failed)?
                .join("session.zip");
            temporary
                .persist_noclobber(target)
                .map_err(|_| AgentSessionError::Failed)?;
            Ok(())
        })
        .await
        .map_err(|_| AgentSessionError::Failed)?
    }

    /// Add only an actually delivered archive to its original live or replayed command card.
    /// Missing artifacts leave native history unchanged; invalid private storage fails safely.
    pub(super) fn decorate(&self, entry: &mut NativeItem) -> Result<(), AgentSessionError> {
        if entry.item["type"] != "provider_command"
            || entry.item["command"] != "export"
            || entry.item["status"] != "success"
        {
            return Ok(());
        }
        let identity = entry.item["id"].as_str().ok_or(AgentSessionError::Failed)?;
        let directory = self.directory(identity);
        let metadata = match directory.join("session.zip").symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(AgentSessionError::Failed),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AgentSessionError::Failed);
        }
        validate_directory(&self.directory)?;
        validate_directory(&directory)?;
        entry.item["file"] = json!({"directory":directory.to_str().ok_or(AgentSessionError::Failed)?,
            "fileName":"session.zip","size":metadata.len(),"mimeType":"application/zip"});
        Ok(())
    }
}

fn private_directory(directory: &Path) -> Result<(), AgentSessionError> {
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
    validate_directory(directory)
}

fn validate_directory(directory: &Path) -> Result<(), AgentSessionError> {
    let metadata = directory
        .symlink_metadata()
        .map_err(|_| AgentSessionError::Failed)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AgentSessionError::Failed);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
