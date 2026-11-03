//! File access coordination and one-use download grants.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::files::ports::{FileError, FileInfo, FileReader, FileSystem};

/// Filesystem use cases and bounded, short-lived download grants.
#[derive(Debug)]
pub struct Files {
    /// Filesystem boundary used by the file use cases.
    pub(crate) filesystem: Box<dyn FileSystem>,
    downloads: BTreeMap<String, DownloadGrant>,
}

#[derive(Debug)]
struct DownloadGrant {
    expires: Instant,
    root: String,
    path: String,
    file_name: String,
    /// The visiting account a guest connection issued this grant for; `None` for the owner.
    visitor: Option<String>,
}

impl Files {
    /// Compose file operations with an isolated filesystem adapter.
    #[must_use]
    pub fn new(filesystem: Box<dyn FileSystem>) -> Self {
        Self {
            filesystem,
            downloads: BTreeMap::new(),
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
                root: info.root.clone(),
                path: info.absolute_path.clone(),
                file_name: info.file_name.clone(),
                visitor: None,
            },
        );
        Ok(info)
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
        let reader = self.filesystem.open(&grant.root, &grant.path)?;
        if reader.info().absolute_path != grant.path {
            return Err(FileError("Download target changed".to_owned()));
        }
        Ok((reader, grant.file_name))
    }
}

#[cfg(test)]
mod tests;
