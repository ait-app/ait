//! Read-only harness evidence. Raw text is for the host sanitizer, never direct client output.

use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use tokio::io::AsyncReadExt;

const FILE_BYTES: u64 = 64 * 1024;
const FILE_COUNT: usize = 3;
const DIRECTORY_ENTRIES: usize = 1024;

/// Native CLI and log locations resolved once from the daemon environment.
#[derive(Debug)]
pub struct HarnessEvidence {
    sources: Vec<Source>,
}

#[derive(Debug)]
struct Source {
    name: &'static str,
    program: PathBuf,
    logs: Option<PathBuf>,
}

impl Default for HarnessEvidence {
    fn default() -> Self {
        Self::configured(|name| std::env::var_os(name))
    }
}

impl HarnessEvidence {
    /// Resolve configured launchers and log roots using `environment`, without I/O.
    /// Returns a read-only collector; custom log roots must be dedicated native log directories.
    #[must_use]
    fn configured(environment: impl Fn(&str) -> Option<OsString>) -> Self {
        let home = environment("HOME")
            .or_else(|| environment("USERPROFILE"))
            .map(PathBuf::from);
        let root = |key, suffix| {
            environment(key)
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(suffix)))
        };
        let sources = [
            ("opencode", "OPENCODE", "opencode", None),
            (
                "codex",
                "CODEX",
                "codex",
                root("CODEX_HOME", ".codex").map(|p| p.join("log")),
            ),
            (
                "claude",
                "CLAUDE",
                "claude",
                root("CLAUDE_CONFIG_DIR", ".claude").map(|p| p.join("debug")),
            ),
            ("deepseek-harness", "DEEPSEEK_HARNESS", "dsh", None),
            ("antigravity", "ANTIGRAVITY", "agy", None),
        ]
        .into_iter()
        .map(|(name, key, fallback, logs)| Source {
            name,
            program: environment(&format!("AIT_SERVER_{key}_BIN"))
                .map_or_else(|| fallback.into(), PathBuf::from),
            logs: environment(&format!("AIT_DIAGNOSTICS_{key}_LOG_DIR"))
                .map(PathBuf::from)
                .or(logs),
        })
        .collect();
        Self { sources }
    }

    /// Read native versions and bounded log excerpts for the inclusive UTC window `start..=end`.
    ///
    /// Returns partial evidence with explicit unavailable/truncation markers. The caller MUST
    /// sanitize this raw text before persisting or sharing it. Filesystem reads run off reactor.
    pub async fn collect(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> String {
        let reports =
            futures_util::future::join_all(self.sources.iter().map(|source| async move {
                let version = probe(&source.program, &["--version"])
                    .await
                    .unwrap_or_else(|error| format!("unavailable ({error})"));
                let mut report = format!("\nHarness: {}\n  Version: {}\n", source.name, version);
                let logs = match &source.logs {
                    Some(path) => Some(path.clone()),
                    None if source.name == "opencode" => {
                        match probe(&source.program, &["debug", "paths", "log"]).await {
                            Ok(text) if Path::new(&text).is_absolute() => Some(PathBuf::from(text)),
                            Ok(_) => {
                                report.push_str("  Log discovery: non-absolute path rejected\n");
                                None
                            }
                            Err(error) => {
                                let _ = writeln!(report, "  Log discovery: {error}");
                                None
                            }
                        }
                    }
                    None => None,
                };
                if let Some(logs) = logs {
                    let excerpt =
                        tokio::task::spawn_blocking(move || collect_directory(&logs, start, end))
                            .await;
                    report
                        .push_str(&excerpt.unwrap_or_else(|_| "  Logs: collector failed\n".into()));
                } else {
                    report.push_str(
                        "  Logs: unavailable; no native log directory discovered or configured\n",
                    );
                }
                report
            }))
            .await;
        reports.concat()
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ProbeError {
    #[error("executable unavailable")]
    Unavailable,
    #[error("probe I/O failed")]
    Io,
    #[error("output exceeded 4 KiB")]
    TooLarge,
    #[error("command exited unsuccessfully")]
    Failed,
    #[error("invalid probe output")]
    InvalidOutput,
    #[error("probe timed out after 2 seconds")]
    Timeout,
}

async fn probe(program: &Path, args: &[&str]) -> Result<String, ProbeError> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ProbeError::Unavailable)?;
    let mut stdout = child.stdout.take().ok_or(ProbeError::Io)?.take(4097);
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut bytes = Vec::new();
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ProbeError::Io)?;
        if bytes.len() > 4096 {
            return Err(ProbeError::TooLarge);
        }
        let status = child.wait().await.map_err(|_| ProbeError::Io)?;
        if !status.success() {
            return Err(ProbeError::Failed);
        }
        let value = String::from_utf8(bytes).map_err(|_| ProbeError::InvalidOutput)?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ProbeError::InvalidOutput);
        }
        Ok(value.to_owned())
    })
    .await
    .unwrap_or(Err(ProbeError::Timeout));
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    }
    result
}

fn collect_directory(directory: &Path, start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    let Ok(entries) = directory.read_dir() else {
        return "  Logs: directory unavailable\n".into();
    };
    let mut files = Vec::new();
    let mut scanned = 0;
    for entry in entries.take(DIRECTORY_ENTRIES + 1) {
        scanned += 1;
        if scanned > DIRECTORY_ENTRIES {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        // Never traverse symlinks, directories, sockets, transcripts or configuration files.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("log" | "txt")
        ) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if DateTime::<Utc>::from(modified) >= start {
            files.push((modified, path));
        }
    }
    files.sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut report = format!(
        "  Log directory: {}\n  Eligible files: {}; selected: {}; directory scan truncated: {}\n",
        directory.display(),
        files.len(),
        files.len().min(FILE_COUNT),
        scanned > DIRECTORY_ENTRIES
    );
    for (_, path) in files.iter().take(FILE_COUNT) {
        let _ = writeln!(
            report,
            "  File: {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        match read_tail(path) {
            Ok((text, truncated)) => {
                let (excerpt, skipped) = window_excerpt(&text, start, end);
                let _ = writeln!(
                    report,
                    "  Tail truncated: {truncated}; unscoped/outside-window lines omitted: {skipped}"
                );
                if excerpt.is_empty() {
                    report.push_str("  No timestamped records in window\n");
                }
                report.push_str(&excerpt);
            }
            Err(_) => report.push_str("  File unavailable\n"),
        }
    }
    report
}

fn read_tail(path: &Path) -> std::io::Result<(String, bool)> {
    let mut file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let offset = file.metadata()?.len().saturating_sub(FILE_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(FILE_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if offset > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Ok((text.to_owned(), offset > 0))
}

fn window_excerpt(text: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> (String, usize) {
    let mut report = String::new();
    let mut omitted = 0;
    // Untimestamped continuations are deliberately omitted: they can contain prompts or payloads.
    for line in text.lines() {
        if timestamp(line).is_some_and(|time| time >= start && time <= end) {
            report.push_str(line);
            report.push('\n');
        } else {
            omitted += 1;
        }
    }
    (report, omitted)
}

fn timestamp(line: &str) -> Option<DateTime<Utc>> {
    // Native ISO timestamps may follow a log level or a JSON field name.
    let start = line.char_indices().find_map(|(index, _)| {
        let suffix = &line[index..];
        (suffix.as_bytes().get(4) == Some(&b'-')
            && suffix.as_bytes().get(7) == Some(&b'-')
            && suffix.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .then_some(index)
    })?;
    let suffix = &line[start..];
    let end = suffix
        .find(['"', '\'', ']', ',', '\t'])
        .unwrap_or(suffix.len());
    let value = &suffix[..end];
    let mut parts = value.split_whitespace();
    let first = parts.next()?;
    let token = if first.len() == 10 {
        Cow::Owned(format!("{first}T{}", parts.next()?))
    } else {
        Cow::Borrowed(first)
    };
    if let Ok(time) = DateTime::parse_from_rfc3339(&token) {
        return Some(time.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(&token, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|time| time.and_utc())
}

#[cfg(test)]
mod tests;
