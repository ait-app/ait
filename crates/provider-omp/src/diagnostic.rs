//! Oh My Pi availability report (D31): `omp --version` checked against Ait's floor, and OMP's
//! profile and roots, without starting a session process.

use std::fmt::Write as _;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use super::{LABEL, OmpClient, history::paths};
use provider_core::ports::agent_session::AgentSessionError;

/// Oldest supported OMP: Ait's documented compatibility floor, not Paseo's 16.3.9, because
/// Ait relies on RPC v2 and the native settlement facts of that release (ADR-194).
const MIN_VERSION: [u64; 3] = [18, 12, 6];
/// Wait for `omp --version`, as Paseo waits.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Most `--version` output read; only its first line is reported.
const MAX_VERSION_BYTES: usize = 1024;
/// Bound of the report, as `AgentClient::diagnostic` requires.
const MAX_REPORT_BYTES: usize = 4096;

/// Report the installed OMP's version and support, its profile, configuration root, agent
/// directory, agent database and XDG roots, as Paseo does. Values are paths and fixed text:
/// no environment values, stderr or native payloads, so no credentials.
pub(super) async fn report(client: &OmpClient) -> String {
    let version = version(client).await;
    let mut report = format!("{LABEL} (OMP)");
    row(&mut report, "Binary", &client.program.to_string_lossy());
    match &version {
        Ok(output) => {
            row(&mut report, "Version", output);
            row(&mut report, "Version support", &support(output));
        }
        Err(reason) => row(&mut report, "Version", &format!("unavailable ({reason})")),
    }
    match paths::roots(client) {
        Ok(roots) => {
            row(
                &mut report,
                "Active profile",
                roots.profile.as_deref().unwrap_or("default"),
            );
            row(&mut report, "Config root", &roots.config.to_string_lossy());
            row(
                &mut report,
                "Agent directory",
                &roots.agent.to_string_lossy(),
            );
            let database = roots.database();
            let found = if database.is_file() {
                "found"
            } else {
                "not found"
            };
            row(
                &mut report,
                "Agent database",
                &format!("{} ({found})", database.to_string_lossy()),
            );
            // As in Paseo, a root without an XDG directory is the configuration root.
            for (label, root) in ["XDG data root", "XDG state root", "XDG cache root"]
                .into_iter()
                .zip(&roots.xdg)
            {
                let root = root.as_deref().unwrap_or(&roots.config);
                row(&mut report, label, &root.to_string_lossy());
            }
        }
        Err(AgentSessionError::Rejected) => row(
            &mut report,
            "Active profile",
            "invalid OMP_PROFILE or PI_PROFILE; OMP refuses to start",
        ),
        Err(_) => row(&mut report, "Config root", "unavailable; no home directory"),
    }
    let end = report.floor_char_boundary(report.len().min(MAX_REPORT_BYTES));
    report.truncate(end);
    report
}

/// Append one `  label: value` line, with control characters replaced so it stays one line.
fn row(report: &mut String, label: &str, value: &str) {
    let value: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect();
    let _ = write!(report, "\n  {label}: {value}");
}

/// Run `omp --version` with the client's environment, without a shell, and return the first
/// line it prints, or why there is none.
async fn version(client: &OmpClient) -> Result<String, &'static str> {
    let mut child = tokio::process::Command::new(&client.program)
        .arg("--version")
        .envs(client.environment.entries())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "could not start")?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("no output")?
        .take(MAX_VERSION_BYTES as u64);
    // Dropping the child on timeout kills it.
    tokio::time::timeout(VERSION_TIMEOUT, async {
        let mut bytes = Vec::with_capacity(64);
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "unreadable output")?;
        let status = child.wait().await.map_err(|_| "no exit status")?;
        if !status.success() {
            return Err("exited unsuccessfully");
        }
        let text = String::from_utf8_lossy(&bytes);
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
            .ok_or("no output")
    })
    .await
    .unwrap_or(Err("timed out"))
}

/// Paseo's support line: the first `major.minor.patch` in `output` compared with
/// [`MIN_VERSION`].
fn support(output: &str) -> String {
    let [major, minor, patch] = MIN_VERSION;
    let minimum = format!("minimum {major}.{minor}.{patch}");
    let Some(installed) = semantic_version(output) else {
        return format!("unknown ({minimum})");
    };
    let verdict = if installed >= MIN_VERSION {
        "supported"
    } else {
        "unsupported"
    };
    let [major, minor, patch] = installed;
    format!("{major}.{minor}.{patch} ({verdict}; {minimum})")
}

/// The first three dot-separated numbers in `text`, such as `18.18.8` in `omp/18.18.8`.
fn semantic_version(text: &str) -> Option<[u64; 3]> {
    text.split(|character: char| !character.is_ascii_digit() && character != '.')
        .find_map(|candidate| {
            let mut parts = candidate.split('.').map(|part| part.parse().ok());
            Some([parts.next()??, parts.next()??, parts.next()??])
        })
}

#[cfg(test)]
mod tests;
