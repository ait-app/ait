mod identity;

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use anyhow::{Context, bail};
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct InstanceLease {
    // Never unlink the lock file: other processes may already hold the same inode open.
    _lock: File,
    pub server_id: Uuid,
    pub instance_id: Uuid,
}

impl InstanceLease {
    pub(crate) fn acquire(directory: &Path) -> anyhow::Result<Self> {
        create_directory(directory)?;
        let directory = directory
            .canonicalize()
            .context("resolve server data directory")?;
        let lock_path = directory.join("instance.lock");
        reject_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .context("open server instance lock")?;
        lock.try_lock()
            .context("server data directory is already in use or cannot be locked")?;
        let server_id =
            identity::load_or_create(&directory).context("load stable server identity")?;
        Ok(Self {
            _lock: lock,
            server_id,
            instance_id: Uuid::new_v4(),
        })
    }
}

impl InstanceLease {
    /// Whether another process currently holds `directory`'s lease, without creating anything.
    /// A brief shared probe only runs after a startup failure, to explain that failure.
    pub(crate) fn held(directory: &Path) -> bool {
        let lock_path = directory.join("instance.lock");
        if reject_symlink(&lock_path).is_err() {
            return false;
        }
        OpenOptions::new()
            .read(true)
            .open(lock_path)
            .is_ok_and(|lock| matches!(lock.try_lock_shared(), Err(TryLockError::WouldBlock)))
    }
}

fn create_directory(directory: &Path) -> anyhow::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .context("create server data directory")
}

fn reject_symlink(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            bail!("server state must be a regular file: {}", path.display());
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("inspect server state file"),
    }
}

#[cfg(test)]
mod tests;
