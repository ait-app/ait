//! Atomic project configuration persistence with optimistic file revisions.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use domain::storage::project::{
    ProjectConfigDocument, ProjectConfigRevision, ProjectConfigStoreError, ProjectConfigWrite,
};
use domain::workspace::provisioning::{LEGACY_PROJECT_CONFIG_FILE_NAME, PROJECT_CONFIG_FILE_NAME};
use model::storage::project::ProjectConfigStore;

const MAX_CONFIG_BYTES: u32 = 4 * 1024 * 1024;

/// Atomic local `ait.json` adapter.
#[derive(Debug, Default)]
pub struct LocalProjectConfigStore;

impl ProjectConfigStore for LocalProjectConfigStore {
    fn config_path(&self, root: &Path) -> std::io::Result<PathBuf> {
        read_path(root)
    }

    fn read(&self, root: &str) -> Result<ProjectConfigDocument, ProjectConfigStoreError> {
        let path = read_path(Path::new(root)).map_err(|_| ProjectConfigStoreError::Invalid)?;
        let Some(revision) = revision(&path).map_err(|_| ProjectConfigStoreError::Invalid)? else {
            return Ok(ProjectConfigDocument {
                config: None,
                revision: None,
            });
        };
        if revision.size > f64::from(MAX_CONFIG_BYTES) {
            return Err(ProjectConfigStoreError::Invalid);
        }
        let file = File::open(path).map_err(|_| ProjectConfigStoreError::Invalid)?;
        let config = serde_json::from_reader(file).map_err(|_| ProjectConfigStoreError::Invalid)?;
        Ok(ProjectConfigDocument {
            config: Some(config),
            revision: Some(revision),
        })
    }

    fn write(
        &self,
        root: &str,
        config: &serde_json::Value,
        expected_revision: Option<ProjectConfigRevision>,
    ) -> Result<ProjectConfigWrite, ProjectConfigStoreError> {
        let root = Path::new(root);
        let path = root.join(PROJECT_CONFIG_FILE_NAME);
        let mut staged =
            tempfile::NamedTempFile::new_in(root).map_err(|_| ProjectConfigStoreError::Write)?;
        serde_json::to_writer_pretty(staged.as_file_mut(), config)
            .map_err(|_| ProjectConfigStoreError::Write)?;
        staged
            .as_file_mut()
            .write_all(b"\n")
            .and_then(|()| staged.as_file_mut().sync_all())
            .map_err(|_| ProjectConfigStoreError::Write)?;
        let current_path = read_path(root).map_err(|_| ProjectConfigStoreError::Write)?;
        let current_revision =
            revision(&current_path).map_err(|_| ProjectConfigStoreError::Write)?;
        if current_revision != expected_revision {
            return Ok(ProjectConfigWrite::Stale { current_revision });
        }
        staged
            .persist(&path)
            .map_err(|_| ProjectConfigStoreError::Write)?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| ProjectConfigStoreError::Write)?;
        let revision = revision(&path)
            .map_err(|_| ProjectConfigStoreError::Write)?
            .ok_or(ProjectConfigStoreError::Write)?;
        Ok(ProjectConfigWrite::Written {
            config: config.clone(),
            revision,
        })
    }
}

// Prefer Ait even when it is invalid; select the legacy file only when Ait is absent.
fn read_path(root: &Path) -> std::io::Result<PathBuf> {
    let path = root.join(PROJECT_CONFIG_FILE_NAME);
    match path.symlink_metadata() {
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(root.join(LEGACY_PROJECT_CONFIG_FILE_NAME))
        }
        Err(error) => Err(error),
    }
}

fn revision(path: &Path) -> std::io::Result<Option<ProjectConfigRevision>> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() {
        return Err(std::io::Error::other("project config is not a file"));
    }
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?;
    let size = u32::try_from(metadata.len())
        .map_err(|_| std::io::Error::other("project config is too large"))?;
    Ok(Some(ProjectConfigRevision {
        mtime_ms: modified.as_secs_f64() * 1000.0,
        size: f64::from(size),
    }))
}

#[cfg(test)]
mod tests;
