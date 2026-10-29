//! Discover an existing npm exec installation without invoking npm or downloading packages.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::local::configuration::executable;

/// Resolve the Unix npm cache using `environment` for exported configuration and HOME.
/// Returns an absolute cache path, or `None` when unavailable or on unsupported platforms.
pub(super) fn cache(environment: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if !cfg!(unix) {
        return None;
    }
    environment("npm_config_cache")
        .filter(|path| !path.is_empty())
        .or_else(|| environment("NPM_CONFIG_CACHE").filter(|path| !path.is_empty()))
        .map(PathBuf::from)
        .or_else(|| environment("HOME").map(|home| PathBuf::from(home).join(".npm")))
        .filter(|path| path.is_absolute())
}

/// Scan at most 256 installations under the npm `cache` without invoking a package manager.
/// Returns the newest executable DSH entrypoint, or `None` when no valid package can be read.
pub(super) fn cached_cli(cache: &Path) -> Option<PathBuf> {
    cache
        .join("_npx")
        .read_dir()
        .ok()?
        .take(256)
        .filter_map(Result::ok)
        .filter_map(|entry| installation(&entry.path()))
        .max()
        .map(|(_, program)| program)
}

fn installation(directory: &Path) -> Option<(SystemTime, PathBuf)> {
    let package = directory.join("node_modules/@deepseek-ai/dsh");
    let manifest = package.join("package.json");
    if manifest.metadata().ok()?.len() > 64 * 1024 {
        return None;
    }
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).ok()?).ok()?;
    if identity["name"] != "@deepseek-ai/dsh" || identity["bin"]["dsh"] != "lib/bin.js" {
        return None;
    }
    let program = package.join("lib/bin.js");
    if !executable(&program) {
        return None;
    }
    Some((package.metadata().ok()?.modified().ok()?, program))
}

#[cfg(all(test, unix))]
mod tests;
