//! Private atomic JSON persistence for push tokens.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use domain::storage::push::PushError;
use model::storage::push::TokenStore;

const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

/// File adapter owned by the server's exclusively leased data directory.
#[derive(Debug)]
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
    /// Select the private token file; no I/O is performed until load/save.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn check_file(&self) -> Result<(), PushError> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                Err(PushError::Invalid)
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(PushError::Io),
        }
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Value, PushError> {
        self.check_file()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            match fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(serde_json::json!({}));
                }
                Err(_) => return Err(PushError::Io),
            }
        }
        let bytes = match crate::File::new(&self.path).read_limited(MAX_DOCUMENT_BYTES) {
            Ok(bytes) => bytes,
            Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(serde_json::json!({}));
            }
            Err(crate::Error::Io(_)) => return Err(PushError::Io),
            Err(crate::Error::TooLarge) => return Err(PushError::Capacity),
        };
        serde_json::from_slice(&bytes).map_err(|_| PushError::Invalid)
    }

    fn save(&self, document: &Value) -> Result<(), PushError> {
        self.check_file()?;
        let mut bytes = serde_json::to_vec_pretty(document).map_err(|_| PushError::Invalid)?;
        if u64::try_from(bytes.len()).map_or(true, |size| size >= MAX_DOCUMENT_BYTES) {
            return Err(PushError::Capacity);
        }
        bytes.push(b'\n');
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or(PushError::Invalid)?;
        fs::create_dir_all(parent).map_err(|_| PushError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|_| PushError::Io)?;
        }
        crate::File::new(&self.path)
            .write(&bytes)
            .map_err(|_| PushError::Io)
    }
}

#[cfg(test)]
mod tests;
