//! Paseo-style cloud host matching, SSH alias resolution and CLI auth probing.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::{
    CommandFamily, ForgeContext, ForgeFailureKind, ForgeRuntimeError, LocalForge, READ_TIMEOUT,
    forge_error, git_optional, parse_remote, run_command,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const NEGATIVE_TTL: Duration = Duration::from_secs(30);
const MAX_HOSTS: usize = 128;

/// Installed CLI adapter selected for a remote host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ForgeKind {
    Github,
    Gitlab,
}

#[derive(Debug)]
struct CachedHost {
    kind: Option<ForgeKind>,
    checked: Instant,
}

/// Bounded authentication-probe results shared by runtime clones.
#[derive(Debug, Default)]
pub(super) struct HostCache(BTreeMap<String, CachedHost>);

/// Resolve `cwd`'s origin through cloud names, SSH aliases and CLI authentication.
/// Returns the adapter, host and project path, or a Git/unsupported-remote error.
pub(super) fn resolve(forge: &LocalForge, cwd: &Path) -> Result<ForgeContext, ForgeRuntimeError> {
    let remote = git_optional(cwd, &["config", "--get", "remote.origin.url"], READ_TIMEOUT)?
        .ok_or_else(|| forge_error(ForgeFailureKind::NoRemote, "No origin remote is configured"))?;
    let location = parse_remote(&remote)
        .filter(|location| valid_host(&location.host))
        .ok_or_else(|| forge_error(ForgeFailureKind::NoRemote, "Unsupported forge remote"))?;
    let ssh = !remote.starts_with("https://") && !remote.starts_with("http://");
    let host = if ssh && cloud_forge(&location.host).is_none() {
        ssh_hostname(forge, &location.host).unwrap_or(location.host)
    } else {
        location.host
    };
    let kind = cloud_forge(&host).or_else(|| probe(forge, cwd, &host)).ok_or_else(|| {
        forge_error(
            ForgeFailureKind::NoRemote,
            "No supported forge is authenticated for this host; use gh or glab auth login --hostname <host>",
        )
    })?;
    Ok(ForgeContext {
        kind,
        host,
        project_path: location.project_path,
    })
}

fn cloud_forge(host: &str) -> Option<ForgeKind> {
    match host {
        "github.com" => Some(ForgeKind::Github),
        "gitlab.com" => Some(ForgeKind::Gitlab),
        _ if host.ends_with(".ghe.com") => Some(ForgeKind::Github),
        _ => None,
    }
}

fn valid_host(host: &str) -> bool {
    host.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn ssh_hostname(forge: &LocalForge, host: &str) -> Option<String> {
    let mut command = Command::new(&forge.ssh_executable);
    command.args(["-G", "--", host]);
    let output = run_command(command, PROBE_TIMEOUT, CommandFamily::Git).ok()?;
    output.stdout.lines().find_map(|line| {
        let (name, value) = line.split_once(' ')?;
        (name == "hostname" && valid_host(value)).then(|| value.to_ascii_lowercase())
    })
}

fn probe(forge: &LocalForge, cwd: &Path, host: &str) -> Option<ForgeKind> {
    // The filesystem worker serializes calls; this cache is also shared by clones.
    if let Ok(cache) = forge.hosts.lock()
        && let Some(cached) = cache.0.get(host)
        && (cached.kind.is_some() || cached.checked.elapsed() < NEGATIVE_TTL)
    {
        return cached.kind;
    }
    let kind = [ForgeKind::Github, ForgeKind::Gitlab]
        .into_iter()
        .find(|kind| {
            let executable = match kind {
                ForgeKind::Github => &forge.executable,
                ForgeKind::Gitlab => &forge.gitlab_executable,
            };
            let mut command = Command::new(executable);
            command
                .args(["auth", "status", "--hostname", host])
                .current_dir(cwd)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GH_PROMPT_DISABLED", "1")
                .env("GLAB_CHECK_UPDATE", "0");
            run_command(command, PROBE_TIMEOUT, CommandFamily::Forge).is_ok()
        });
    if let Ok(mut cache) = forge.hosts.lock() {
        if cache.0.len() >= MAX_HOSTS {
            cache.0.clear();
        }
        cache.0.insert(
            host.to_owned(),
            CachedHost {
                kind,
                checked: Instant::now(),
            },
        );
    }
    kind
}

#[cfg(test)]
mod tests;
