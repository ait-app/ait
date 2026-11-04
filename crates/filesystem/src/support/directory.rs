//! Resolution of client-supplied checkout directories.

use std::path::PathBuf;

/// Why a client-supplied checkout directory cannot be used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DirectoryError {
    /// The path starts with `~` and `$HOME` is unset.
    #[error("Home directory is unavailable")]
    HomeUnavailable,
    /// The path does not resolve to an existing location.
    #[error("{0}")]
    Unresolvable(std::io::Error),
    /// The path resolves to something other than a directory.
    #[error("Checkout cwd is not a directory")]
    NotDirectory,
}

/// Expand a leading `~` against `$HOME` and canonicalize the result to an existing directory.
///
/// # Errors
/// Returns [`DirectoryError`] when `$HOME` is needed but unset, the path cannot be
/// canonicalized, or it is not a directory.
pub(crate) fn existing_directory(value: &str) -> Result<PathBuf, DirectoryError> {
    let path = if value == "~" {
        home()?
    } else if let Some(relative) = value.strip_prefix("~/") {
        home()?.join(relative)
    } else {
        PathBuf::from(value)
    };
    let path = path.canonicalize().map_err(DirectoryError::Unresolvable)?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(DirectoryError::NotDirectory)
    }
}

fn home() -> Result<PathBuf, DirectoryError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(DirectoryError::HomeUnavailable)
}
