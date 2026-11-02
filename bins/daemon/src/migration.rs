//! Exclusive, recoverable migration of legacy desktop projects and Agent history.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::instance::InstanceLease;

const MIGRATED_DIRECTORIES: [&str; 2] = ["agents", "projects"];
const TIMELINE_TABLES: [&str; 10] = [
    "timelines",
    "entries",
    "retired_entries",
    "progress",
    "input_receipts",
    "provider_subagents",
    "projection_heads",
    "display_entries",
    "display_ranges",
    "display_sources",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DataSummary {
    agents: u64,
    projects: u64,
    workspaces: u64,
    timeline_rows: u64,
}

impl DataSummary {
    fn has_data(&self) -> bool {
        self.agents > 0 || self.projects > 0 || self.workspaces > 0 || self.timeline_rows > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct MigrationReport {
    source: PathBuf,
    destination: PathBuf,
    backup: PathBuf,
    summary: DataSummary,
    copied_files: u64,
    copied_bytes: u64,
}

#[derive(Debug, Default)]
struct CopyStats {
    files: u64,
    bytes: u64,
}

/// Copy legacy user data into an empty destination while both daemon directories are locked.
///
/// The destination keeps its own identity and credentials. Existing destination `agents` and
/// `projects` directories are moved to the reported backup directory before the copied data is
/// published. The source is never modified.
///
/// # Errors
/// Returns an error when either daemon directory is active, the source is empty, the destination
/// contains user records, a state entry is unsafe, or copying and verification fail.
pub(super) fn migrate(source: &Path, destination: &Path, json: bool) -> anyhow::Result<()> {
    let report = migrate_data(source, destination)?;
    if json {
        writeln!(std::io::stdout(), "{}", serde_json::to_string(&report)?)?;
    } else {
        writeln!(
            std::io::stdout(),
            "Migrated {} files ({} bytes) from {} to {}. Previous destination data: {}",
            report.copied_files,
            report.copied_bytes,
            report.source.display(),
            report.destination.display(),
            report.backup.display()
        )?;
    }
    Ok(())
}

fn migrate_data(source: &Path, destination: &Path) -> anyhow::Result<MigrationReport> {
    if !source.is_dir() {
        bail!("legacy data directory does not exist: {}", source.display());
    }
    fs::create_dir_all(destination).context("create desktop data directory")?;
    let source = source
        .canonicalize()
        .context("resolve legacy data directory")?;
    let destination = destination
        .canonicalize()
        .context("resolve desktop data directory")?;
    if source == destination || source.starts_with(&destination) || destination.starts_with(&source)
    {
        bail!("source and destination data directories must be independent");
    }

    let (_first_lease, _second_lease) = if source < destination {
        (
            InstanceLease::acquire(&source).context("lock legacy data directory")?,
            InstanceLease::acquire(&destination).context("lock desktop data directory")?,
        )
    } else {
        (
            InstanceLease::acquire(&destination).context("lock desktop data directory")?,
            InstanceLease::acquire(&source).context("lock legacy data directory")?,
        )
    };

    let source_summary = summarize(&source).context("inspect legacy data")?;
    if !source_summary.has_data() {
        bail!("legacy data directory contains no projects or Agent history");
    }
    if summarize(&destination)
        .context("inspect desktop data")?
        .has_data()
    {
        bail!("desktop data directory already contains projects or Agent history");
    }

    let parent = destination
        .parent()
        .context("desktop data directory has no parent")?;
    let identifier = uuid::Uuid::new_v4();
    let staging = parent.join(format!(".ait-data-migration-{identifier}"));
    let backup = parent.join(format!(".ait-server-desktop-before-migration-{identifier}"));
    fs::create_dir(&staging).context("create migration staging directory")?;
    fs::create_dir(&backup).context("create migration backup directory")?;

    let result = stage_and_publish(&source, &destination, &staging, &backup, &source_summary);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    let stats = result?;
    fs::remove_dir(&staging).context("remove empty migration staging directory")?;

    Ok(MigrationReport {
        source,
        destination,
        backup,
        summary: source_summary,
        copied_files: stats.files,
        copied_bytes: stats.bytes,
    })
}

fn stage_and_publish(
    source: &Path,
    destination: &Path,
    staging: &Path,
    backup: &Path,
    source_summary: &DataSummary,
) -> anyhow::Result<CopyStats> {
    let mut stats = CopyStats::default();
    for name in MIGRATED_DIRECTORIES {
        let entry = source.join(name);
        if entry.exists() {
            copy_tree(&entry, &staging.join(name), &mut stats)
                .with_context(|| format!("stage legacy {name}"))?;
        }
    }

    let mut displaced = Vec::new();
    let mut installed = Vec::new();
    let publication = (|| -> anyhow::Result<()> {
        for name in MIGRATED_DIRECTORIES {
            let current = destination.join(name);
            if current.exists() {
                fs::rename(&current, backup.join(name))
                    .with_context(|| format!("back up desktop {name}"))?;
                displaced.push(name);
            }
            let prepared = staging.join(name);
            if prepared.exists() {
                fs::rename(&prepared, &current)
                    .with_context(|| format!("publish migrated {name}"))?;
                installed.push(name);
            }
        }
        sync_directory(destination).context("sync migrated desktop data directory")?;
        sync_directory(backup).context("sync desktop data backup directory")?;
        sync_directory(
            destination
                .parent()
                .context("desktop data directory has no parent")?,
        )
        .context("sync migration parent directory")?;
        Ok(())
    })();
    if let Err(error) = publication {
        rollback_publication(destination, backup, &installed, &displaced);
        return Err(error);
    }

    match summarize(destination).context("verify migrated desktop data") {
        Ok(summary) if &summary == source_summary => Ok(stats),
        Ok(_) => {
            rollback_publication(destination, backup, &installed, &displaced);
            bail!("migrated data summary does not match the legacy source")
        }
        Err(error) => {
            rollback_publication(destination, backup, &installed, &displaced);
            Err(error)
        }
    }
}

fn rollback_publication(destination: &Path, backup: &Path, installed: &[&str], displaced: &[&str]) {
    for name in installed.iter().rev() {
        let _ = fs::remove_dir_all(destination.join(name));
    }
    for name in displaced.iter().rev() {
        let _ = fs::rename(backup.join(name), destination.join(name));
    }
    let _ = sync_directory(destination);
    if let Some(parent) = destination.parent() {
        let _ = sync_directory(parent);
    }
}

fn copy_tree(source: &Path, destination: &Path, stats: &mut CopyStats) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source).context("inspect migration source entry")?;
    if metadata.file_type().is_symlink() {
        bail!(
            "migration source contains a symbolic link: {}",
            source.display()
        );
    }
    if metadata.is_file() {
        let bytes = fs::copy(source, destination).context("copy migration source file")?;
        File::open(destination)
            .and_then(|file| file.sync_all())
            .context("sync copied migration file")?;
        stats.files += 1;
        stats.bytes += bytes;
        return Ok(());
    }
    if !metadata.is_dir() {
        bail!(
            "migration source contains an unsupported entry: {}",
            source.display()
        );
    }
    fs::create_dir(destination).context("create migration destination directory")?;
    fs::set_permissions(destination, metadata.permissions())
        .context("copy migration directory permissions")?;
    let mut entries = fs::read_dir(source)
        .context("read migration source directory")?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()), stats)?;
    }
    sync_directory(destination)?;
    Ok(())
}

fn summarize(directory: &Path) -> anyhow::Result<DataSummary> {
    Ok(DataSummary {
        agents: json_array_len(&directory.join("agents/agents.json"))?,
        projects: json_array_len(&directory.join("projects/projects.json"))?,
        workspaces: json_array_len(&directory.join("projects/workspaces.json"))?,
        timeline_rows: timeline_rows(&directory.join("agents/timeline.sqlite3"))?,
    })
}

fn json_array_len(path: &Path) -> anyhow::Result<u64> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let records = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes)
        .with_context(|| format!("decode {}", path.display()))?;
    u64::try_from(records.len()).context("record count exceeds u64")
}

fn timeline_rows(path: &Path) -> anyhow::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("open timeline database read-only")?;
    TIMELINE_TABLES.iter().try_fold(0_u64, |total, table| {
        let exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get::<_, bool>(0),
            )
            .context("inspect timeline schema")?;
        if !exists {
            return Ok(total);
        }
        let count = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get::<_, u64>(0)
            })
            .with_context(|| format!("count timeline table {table}"))?;
        total
            .checked_add(count)
            .context("timeline row count overflow")
    })
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> anyhow::Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .context("sync migration directory")
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
