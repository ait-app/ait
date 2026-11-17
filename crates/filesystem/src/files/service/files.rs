//! File access coordination and one-use download grants.

use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::files::ports::{FileError, FileInfo, FileKind, FileReader, FileSystem};

/// Filesystem use cases and bounded, short-lived download grants.
#[derive(Debug)]
pub struct Files {
    /// Filesystem boundary used by the file use cases.
    pub(crate) filesystem: Box<dyn FileSystem>,
    downloads: BTreeMap<String, DownloadGrant>,
    generated_slots: Arc<Semaphore>,
}

#[derive(Debug)]
struct DownloadGrant {
    expires: Instant,
    source: DownloadSource,
    file_name: String,
    /// The visiting account a guest connection issued this grant for; `None` for the owner.
    visitor: Option<String>,
    expiry_guard: Option<DropGuard>,
}

#[derive(Debug)]
enum DownloadSource {
    File {
        root: String,
        path: String,
    },
    Generated {
        agent_id: String,
        reader: Box<GeneratedReader>,
    },
}

#[derive(Debug)]
struct GeneratedReader {
    file: std::fs::File,
    info: FileInfo,
    // Hold capacity through the entire transfer, not just token issuance.
    _permit: OwnedSemaphorePermit,
}

impl Read for GeneratedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

impl FileReader for GeneratedReader {
    fn info(&self) -> &FileInfo {
        &self.info
    }

    fn verify(&self) -> Result<(), FileError> {
        if self.file.metadata()?.len() != self.info.size {
            return Err(FileError(
                "Generated download changed during transfer".to_owned(),
            ));
        }
        Ok(())
    }
}

impl Files {
    /// Compose file operations with an isolated filesystem adapter.
    #[must_use]
    pub fn new(filesystem: Box<dyn FileSystem>) -> Self {
        Self {
            filesystem,
            downloads: BTreeMap::new(),
            generated_slots: Arc::new(Semaphore::new(8)),
        }
    }

    /// Issue a one-minute, single-use grant for a currently readable file.
    ///
    /// # Errors
    /// Rejects invalid files or exhaustion of the 256 outstanding grant limit.
    pub(crate) fn issue_download(
        &mut self,
        token: String,
        cwd: &str,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        let now = Instant::now();
        self.downloads.retain(|_, grant| grant.expires > now);
        if self.downloads.len() >= 256 || self.downloads.contains_key(&token) {
            return Err(FileError("Download token capacity exhausted".to_owned()));
        }
        let reader = self.filesystem.open(cwd, path)?;
        let info = reader.info().clone();
        self.downloads.insert(
            token,
            DownloadGrant {
                expires: now + Duration::from_secs(60),
                source: DownloadSource::File {
                    root: info.root.clone(),
                    path: info.absolute_path.clone(),
                },
                file_name: info.file_name.clone(),
                visitor: None,
                expiry_guard: None,
            },
        );
        Ok(info)
    }

    /// Register a completed, bounded generated artifact in the existing single-use grant store.
    ///
    /// Returns a cancellation signal that ends the API expiry task as soon as the grant is consumed
    /// or released. The API expires the grant after one minute even when no later request arrives.
    /// Artifact ownership, including the eight-transfer capacity permit, follows the reader.
    ///
    /// # Errors
    /// Rejects oversized/incomplete artifacts, duplicate tokens, or exhausted grant/transfer capacity.
    pub fn issue_artifact(
        &mut self,
        token: String,
        mut artifact: model::download::GeneratedArtifact,
    ) -> Result<CancellationToken, FileError> {
        let now = Instant::now();
        self.downloads.retain(|_, grant| grant.expires > now);
        if self.downloads.len() >= 256 || self.downloads.contains_key(&token) {
            return Err(FileError("Download token capacity exhausted".to_owned()));
        }
        if artifact.size > model::download::MAX_ARTIFACT_BYTES
            || artifact.file.metadata()?.len() != artifact.size
            || artifact.file.stream_position()? != 0
        {
            return Err(FileError(
                "Invalid generated download size or position".to_owned(),
            ));
        }
        let permit = Arc::clone(&self.generated_slots)
            .try_acquire_owned()
            .map_err(|_| FileError("Generated download capacity exhausted".to_owned()))?;
        let finished = CancellationToken::new();
        let info = FileInfo {
            // Generated artifacts have no publicly addressable filesystem location.
            root: String::new(),
            absolute_path: String::new(),
            path: artifact.file_name.clone(),
            file_name: artifact.file_name.clone(),
            mime_type: artifact.mime_type,
            kind: FileKind::Text,
            size: artifact.size,
            modified_at: String::new(),
            revision: token.clone(),
        };
        self.downloads.insert(
            token,
            DownloadGrant {
                expires: now + Duration::from_secs(60),
                source: DownloadSource::Generated {
                    agent_id: artifact.agent_id,
                    reader: Box::new(GeneratedReader {
                        file: artifact.file,
                        info,
                        _permit: permit,
                    }),
                },
                file_name: artifact.file_name,
                visitor: None,
                expiry_guard: Some(finished.clone().drop_guard()),
            },
        );
        Ok(finished)
    }

    /// Release an unconsumed artifact only when its owning agent matches.
    /// Returns false for missing, already consumed, or different-agent grants.
    pub fn release_artifact(&mut self, agent_id: &str, token: &str) -> bool {
        let matches = self.downloads.get(token).is_some_and(|grant| {
            matches!(&grant.source, DownloadSource::Generated { agent_id: owner, .. } if owner == agent_id)
        });
        matches && self.downloads.remove(token).is_some()
    }

    /// Bind an unconsumed, unbound grant to the visiting account it was issued for.
    /// Returns false when the grant is gone or already bound.
    pub fn bind_download(&mut self, token: &str, visitor: &str) -> bool {
        match self.downloads.get_mut(token) {
            Some(grant) if grant.visitor.is_none() => {
                grant.visitor = Some(visitor.to_owned());
                true
            }
            _ => false,
        }
    }

    /// Consume a download grant, returning its reader and original download name.
    ///
    /// `downloader` is the visiting account the center asserted for a guest download, or
    /// `None` for the owner. A grant bound to a visitor serves only that visitor, and an
    /// owner's grant serves no visitor. A mismatch leaves the grant for its holder.
    ///
    /// # Errors
    /// Rejects unknown, reused, expired, mismatched, removed, or newly outside paths.
    pub fn consume_download(
        &mut self,
        token: &str,
        downloader: Option<&str>,
    ) -> Result<(Box<dyn FileReader>, String), FileError> {
        if self
            .downloads
            .get(token)
            .is_some_and(|grant| grant.visitor.as_deref() != downloader)
        {
            return Err(FileError("Invalid or expired token".to_owned()));
        }
        let grant = self
            .downloads
            .remove(token)
            .filter(|grant| grant.expires > Instant::now())
            .ok_or_else(|| FileError("Invalid or expired token".to_owned()))?;
        drop(grant.expiry_guard);
        let reader: Box<dyn FileReader> = match grant.source {
            DownloadSource::File { root, path } => {
                let reader = self.filesystem.open(&root, &path)?;
                if reader.info().absolute_path != path {
                    return Err(FileError("Download target changed".to_owned()));
                }
                reader
            }
            DownloadSource::Generated { reader, .. } => reader,
        };
        Ok((reader, grant.file_name))
    }
}

#[cfg(test)]
mod tests;
