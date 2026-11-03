//! Ask npm for effective installation locations instead of guessing user directories.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::local::{configuration::executable, metadata_process};

/// Effective Unix npm installation and cache locations.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Configuration {
    /// npm's resolved global installation prefix.
    pub(super) prefix: PathBuf,
    /// npm's resolved cache directory, including user and global configuration.
    pub(super) cache: PathBuf,
}

/// Find npm in `paths` and request only its effective prefix and cache.
/// Returns `None` when npm is absent, fails, times out, or returns invalid paths.
pub(super) fn read(paths: &[PathBuf]) -> Option<Configuration> {
    if !cfg!(unix) {
        return None;
    }
    let npm = paths
        .iter()
        .map(|path| path.join("npm"))
        .find(|path| executable(path))?;
    // Provider assembly is synchronous and may already run inside a Tokio runtime.
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime.block_on(query(&npm, Duration::from_secs(3)))
    })
    .join()
    .ok()?
}

async fn query(npm: &Path, deadline: Duration) -> Option<Configuration> {
    let mut command = Command::new(npm);
    // Global mode excludes project npmrc; only these two path values reach stdout.
    command.args(["--global", "config", "get", "prefix", "cache"]);
    let output = tokio::time::timeout(deadline, metadata_process::output(&mut command))
        .await
        .ok()?
        .ok()?;
    parse(&output)
}

fn parse(output: &str) -> Option<Configuration> {
    if output.len() > 64 * 1024 {
        return None;
    }
    let mut prefix = None;
    let mut cache = None;
    for line in output.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=')?;
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return None;
        }
        match key {
            "prefix" if prefix.is_none() => prefix = Some(path),
            "cache" if cache.is_none() => cache = Some(path),
            _ => return None,
        }
    }
    Some(Configuration {
        prefix: prefix?,
        cache: cache?,
    })
}

#[cfg(all(test, unix))]
mod tests;
