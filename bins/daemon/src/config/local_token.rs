//! Private local API credential for all daemon startup paths.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, ensure};
use secrecy::{ExposeSecret, SecretString};

/// Restore a local credential or atomically publish a new one without replacing a racing writer.
/// Returns an error for inaccessible, non-private, or invalid credential storage.
pub(super) fn load_or_create(directory: &Path) -> anyhow::Result<SecretString> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .context("create daemon data directory")?;
    let path = directory.join("local-api.token");
    if let Some(token) = read(&path)? {
        return Ok(token);
    }
    // Two independent UUID v4 values provide 244 random bits without another entropy dependency.
    let token = SecretString::from(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .context("create private local API credential")?;
    temporary.write_all(token.expose_secret().as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => {
            File::open(directory)?.sync_all()?;
            Ok(token)
        }
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read(&path)?.context("local API credential disappeared during startup")
        }
        Err(error) => Err(error.error).context("save local API credential"),
    }
}

fn read(path: &Path) -> anyhow::Result<Option<SecretString>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("open local API credential"),
    };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "local API credential must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode().trailing_zeros() >= 6,
            "local API credential must be private (chmod 600 local-api.token)"
        );
    }
    let mut token = String::new();
    file.take(257)
        .read_to_string(&mut token)
        .context("read local API credential")?;
    ensure!(
        (32..=256).contains(&token.len()) && token.bytes().all(|byte| (33..=126).contains(&byte)),
        "invalid saved local API credential"
    );
    Ok(Some(token.into()))
}

#[cfg(all(test, unix))]
mod tests;
