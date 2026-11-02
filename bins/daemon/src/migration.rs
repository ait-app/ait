//! Exclusive, recoverable migration of legacy desktop projects and Agent history.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::instance::InstanceLease;

const MIGRATED_DIRECTORIES: [&str; 2] = ["agents", "projects"];
pub(super) const JOURNAL_FILENAME: &str = ".legacy-data-migration.json";
const LEGACY_WORKTREE_ROOTS_FILENAME: &str = "legacy-worktree-roots.json";
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationJournal {
    source: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
}

/// Refuse normal daemon startup while a prior migration still needs recovery.
pub(super) fn ensure_complete(destination: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(destination.join(JOURNAL_FILENAME)) {
        Ok(_) => {
            bail!(
                "desktop data migration is incomplete; rerun migrate-data before starting the daemon"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect migration journal"),
    }
    Ok(())
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
    create_private_directory(destination, true).context("create desktop data directory")?;
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

    recover_incomplete_migration(&source, &destination)?;

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
    create_private_directory(&staging, false).context("create migration staging directory")?;
    create_private_directory(&backup, false).context("create migration backup directory")?;

    let staged = stage_data(&source, &staging);
    let stats = match staged {
        Ok(stats) => stats,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_dir_all(&backup);
            return Err(error);
        }
    };
    let journal = MigrationJournal {
        source: source.clone(),
        staging: staging.clone(),
        backup: backup.clone(),
    };
    if let Err(error) = write_private_json(&destination.join(JOURNAL_FILENAME), &journal)
        .context("persist migration journal")
    {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&backup);
        return Err(error);
    }
    if let Err(error) = publish_staged(&destination, &staging, &backup, &source_summary) {
        let recovery = recover_incomplete_migration(&source, &destination);
        return match recovery {
            Ok(()) => Err(error),
            Err(recovery) => Err(error.context(format!("migration rollback failed: {recovery:#}"))),
        };
    }
    fs::remove_file(destination.join(JOURNAL_FILENAME)).context("finish migration journal")?;
    sync_directory(&destination).context("sync completed migration")?;
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

fn stage_data(source: &Path, staging: &Path) -> anyhow::Result<CopyStats> {
    let mut stats = CopyStats::default();
    for name in MIGRATED_DIRECTORIES {
        let entry = source.join(name);
        if entry.exists() {
            copy_tree(&entry, &staging.join(name), &mut stats)
                .with_context(|| format!("stage legacy {name}"))?;
        }
    }
    stage_legacy_worktree_roots(source, staging, &mut stats)?;
    Ok(stats)
}

fn publish_staged(
    destination: &Path,
    staging: &Path,
    backup: &Path,
    source_summary: &DataSummary,
) -> anyhow::Result<()> {
    for name in MIGRATED_DIRECTORIES {
        let current = destination.join(name);
        if current.exists() {
            fs::rename(&current, backup.join(name))
                .with_context(|| format!("back up desktop {name}"))?;
        }
        let prepared = staging.join(name);
        if prepared.exists() {
            fs::rename(&prepared, &current).with_context(|| format!("publish migrated {name}"))?;
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

    match summarize(destination).context("verify migrated desktop data") {
        Ok(summary) if &summary == source_summary => Ok(()),
        Ok(_) => bail!("migrated data summary does not match the legacy source"),
        Err(error) => Err(error),
    }
}

fn stage_legacy_worktree_roots(
    source: &Path,
    staging: &Path,
    stats: &mut CopyStats,
) -> anyhow::Result<()> {
    let mut roots =
        read_legacy_worktree_roots(&source.join("projects").join(LEGACY_WORKTREE_ROOTS_FILENAME))?;
    let source_worktrees = source.join("worktrees");
    if source_worktrees.is_dir() {
        roots.push(
            source_worktrees
                .canonicalize()
                .context("resolve legacy worktree root")?,
        );
    }
    roots.sort();
    roots.dedup();
    if roots.is_empty() {
        return Ok(());
    }
    let projects = staging.join("projects");
    if !projects.exists() {
        create_private_directory(&projects, false).context("create staged projects directory")?;
    }
    let file = projects.join(LEGACY_WORKTREE_ROOTS_FILENAME);
    let previous = fs::metadata(&file).ok();
    let previous_bytes = previous.as_ref().map_or(0, fs::Metadata::len);
    if previous.is_some() {
        fs::remove_file(&file).context("replace staged legacy worktree roots")?;
    }
    write_private_json(&file, &roots).context("stage legacy worktree roots")?;
    if previous.is_none() {
        stats.files += 1;
    }
    stats.bytes = stats.bytes.saturating_sub(previous_bytes);
    stats.bytes += fs::metadata(file)
        .context("inspect staged legacy worktree roots")?
        .len();
    Ok(())
}

fn read_legacy_worktree_roots(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn recover_incomplete_migration(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let journal_path = destination.join(JOURNAL_FILENAME);
    if let Ok(metadata) = fs::symlink_metadata(&journal_path)
        && metadata.file_type().is_symlink()
    {
        bail!("migration journal must not be a symbolic link");
    }
    let bytes = match fs::read(&journal_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("read migration journal"),
    };
    let journal: MigrationJournal =
        serde_json::from_slice(&bytes).context("decode migration journal")?;
    if journal.source != source {
        bail!(
            "incomplete migration belongs to a different source: {}",
            journal.source.display()
        );
    }
    validate_transaction_directory(destination, &journal.staging, ".ait-data-migration-")?;
    validate_transaction_directory(
        destination,
        &journal.backup,
        ".ait-server-desktop-before-migration-",
    )?;

    for name in MIGRATED_DIRECTORIES.into_iter().rev() {
        let current = destination.join(name);
        let prepared = journal.staging.join(name);
        let previous = journal.backup.join(name);
        if previous.exists() {
            if current.exists() {
                fs::remove_dir_all(&current)
                    .with_context(|| format!("remove incomplete migrated {name}"))?;
            }
            fs::rename(&previous, &current)
                .with_context(|| format!("restore previous desktop {name}"))?;
        } else if !prepared.exists() && current.exists() {
            fs::remove_dir_all(&current)
                .with_context(|| format!("remove incomplete new desktop {name}"))?;
        }
    }
    if journal.staging.exists() {
        fs::remove_dir_all(&journal.staging).context("remove incomplete migration staging")?;
    }
    if journal.backup.exists() {
        fs::remove_dir_all(&journal.backup).context("remove incomplete migration backup")?;
    }
    fs::remove_file(&journal_path).context("remove recovered migration journal")?;
    sync_directory(destination).context("sync recovered desktop data")?;
    if let Some(parent) = destination.parent() {
        sync_directory(parent).context("sync recovered migration parent")?;
    }
    Ok(())
}

fn validate_transaction_directory(
    destination: &Path,
    candidate: &Path,
    prefix: &str,
) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("desktop data directory has no parent")?;
    let valid_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(prefix));
    if candidate.parent() != Some(parent) || !valid_name {
        bail!("migration journal contains an unsafe transaction directory");
    }
    if let Ok(metadata) = fs::symlink_metadata(candidate)
        && metadata.file_type().is_symlink()
    {
        bail!("migration transaction directory must not be a symbolic link");
    }
    Ok(())
}

fn create_private_directory(path: &Path, recursive: bool) -> anyhow::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if fs::metadata(path)?.mode() & 0o777 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

fn write_private_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_directory(path.parent().context("private state file has no parent")?)?;
    Ok(())
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
