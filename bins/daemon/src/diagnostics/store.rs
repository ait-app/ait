//! Bounded local incident file persistence; only Ait-owned files are eligible for cleanup.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

/// Maximum bytes in one sanitized incident report.
pub(crate) const REPORT_LIMIT: usize = 256 * 1024;

const COUNT_LIMIT: usize = 20;
const BYTE_LIMIT: u64 = 5 * 1024 * 1024;
const MAX_AGE: chrono::Duration = chrono::Duration::days(7);

/// Save already sanitized `report` in `directory`, then enforce retention at `now`.
/// # Errors
/// Returns directory, file-write or retention I/O errors, or rejects oversized reports.
pub(crate) fn save(
    directory: &Path,
    report: &str,
    now: DateTime<Utc>,
) -> Result<(), persistence::Error> {
    if report.len() > REPORT_LIMIT {
        return Err(persistence::Error::TooLarge);
    }
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = directory.join(format!("incident-{}.txt", uuid::Uuid::new_v4()));
    persistence::File::new(path).write(report.as_bytes())?;
    prune(directory, now)?;
    Ok(())
}

fn files(directory: &Path) -> std::io::Result<Vec<(SystemTime, PathBuf, u64)>> {
    let entries = match directory.read_dir() {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix("incident-"))
            .and_then(|name| name.strip_suffix(".txt"))
        else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() || !entry.file_type()?.is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        files.push((metadata.modified()?, entry.path(), metadata.len()));
    }
    files.sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    Ok(files)
}

/// Remove expired or excess Ait-owned incidents in `directory` relative to `now`.
/// # Errors
/// Returns directory enumeration, metadata or removal errors.
pub(crate) fn prune(directory: &Path, now: DateTime<Utc>) -> std::io::Result<()> {
    let mut bytes = 0;
    let mut retained = 0;
    for (modified, path, size) in files(directory)? {
        if retained >= COUNT_LIMIT
            || size > BYTE_LIMIT - bytes
            || DateTime::<Utc>::from(modified) < now - MAX_AGE
        {
            std::fs::remove_file(path)?;
        } else {
            bytes += size;
            retained += 1;
        }
    }
    Ok(())
}

/// Read at most three bounded incidents from `directory`, including unavailable markers.
/// Returns stored text for the host to sanitize again before sharing.
#[must_use]
pub(crate) fn recent(directory: &Path) -> String {
    let Ok(files) = files(directory) else {
        return "\nSaved incidents unavailable\n".into();
    };
    let mut report = format!("\nSaved incidents: {} (latest 3 included)\n", files.len());
    for (_, path, _) in files.into_iter().take(3) {
        report.push_str("\n--- Saved incident ---\n");
        let mut text = String::new();
        let result = std::fs::File::open(path)
            .and_then(|file| file.take(REPORT_LIMIT as u64).read_to_string(&mut text));
        if result.is_ok() {
            report.push_str(&text);
        } else {
            report.push_str("Saved incident unreadable\n");
        }
    }
    report
}

#[cfg(test)]
mod tests;
