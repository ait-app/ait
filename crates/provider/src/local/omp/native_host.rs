//! Verified private OMP native host runtime: bundled Bun plus the patched official SDK.
//!
//! The runtime is installed out of tree by `runtimes/omp-native-host/install.mjs` and
//! described by its `manifest.json`. It runs the official SDK from source under a pinned
//! Bun, so the OMP data directory, credentials and session files remain exactly the ones
//! the official binary uses — only the executable that reads them changes.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Installation directory relative to `data_dir` for the current adapter generation.
const INSTALL_PATH: [&str; 4] = ["providers", "omp-native-host", "1", "manifest.json"];
/// Bundle directory shipped beside a packaged daemon or inside the desktop resources.
const BUNDLE_PATH: &str = "provider-runtimes/omp-native-host/1";
/// Manifest schema this adapter understands.
const SCHEMA_VERSION: u64 = 1;
const ADAPTER: &str = "ait-omp-native-host";
const ADAPTER_VERSION: u64 = 1;
/// Patch marker: present only in a runtime whose official SDK carries the bridge.
const HELLO_MODULE: &str =
    "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-hello.ts";

/// A verified runtime. Constructed only from a manifest that passed every check.
#[derive(Debug, Clone)]
pub struct NativeHost {
    bun: PathBuf,
    entrypoint: PathBuf,
}

impl NativeHost {
    /// Bundled Bun executable that starts the patched SDK.
    #[must_use]
    pub fn bun(&self) -> &Path {
        &self.bun
    }

    /// Patched official SDK entrypoint, passed to Bun as its first argument.
    #[must_use]
    pub fn entrypoint(&self) -> &Path {
        &self.entrypoint
    }
}

/// Resolve the private runtime for `data_dir`.
///
/// Candidates are tried in order: `AIT_OMP_NATIVE_HOST` (an installation root or manifest),
/// the managed location under the daemon's data directory, `AIT_SERVER_OMP_NATIVE_HOST_BUNDLE`,
/// and finally the bundle shipped beside the running daemon. Reading a packaged bundle avoids
/// copying a whole runtime into user state on first launch.
///
/// Returns `None` when no candidate verifies, so the caller falls back to the official binary
/// and declares the bridge capability unavailable. A manifest is never partially trusted: a
/// bad runtime degrades instead of launching an unverified program.
pub fn discover(
    data_dir: &Path,
    environment: &dyn Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<NativeHost> {
    let mut candidates = Vec::new();
    if let Some(explicit) = environment("AIT_OMP_NATIVE_HOST").map(PathBuf::from) {
        candidates.push(manifest_of(&explicit));
    }
    candidates.push(data_dir.join(INSTALL_PATH.join("/")));
    if let Some(bundle) = environment("AIT_SERVER_OMP_NATIVE_HOST_BUNDLE").map(PathBuf::from) {
        candidates.push(manifest_of(&bundle));
    }
    candidates.extend(bundled());
    for manifest in candidates {
        match load(&manifest) {
            Ok(host) => {
                tracing::info!(
                    manifest = %manifest.display(),
                    "Using the OMP native host runtime"
                );
                return Some(host);
            }
            Err(reason) => {
                // Only a manifest that exists represents a real installation attempt; a missing
                // candidate is the ordinary "not installed" case and stays quiet.
                if manifest.exists() {
                    tracing::warn!(
                        manifest = %manifest.display(),
                        reason,
                        "Ignoring the OMP native host runtime; OMP will run the official binary without the Ait bridge"
                    );
                }
            }
        }
    }
    None
}

/// A directory resolves to its manifest; a file is treated as the manifest itself.
fn manifest_of(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join("manifest.json")
    } else {
        path.to_path_buf()
    }
}

/// Bundle locations that ship with a packaged daemon or desktop app.
fn bundled() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        roots.push(directory.join(BUNDLE_PATH).join("manifest.json"));
        // The desktop app keeps its resources one level above the daemon binary's directory.
        roots.push(
            directory
                .join("resources")
                .join(BUNDLE_PATH)
                .join("manifest.json"),
        );
        roots.push(
            directory
                .join("Resources")
                .join(BUNDLE_PATH)
                .join("manifest.json"),
        );
    }
    roots
}

/// Read and verify one manifest. Returns a caller-safe reason on any mismatch.
fn load(manifest: &Path) -> Result<NativeHost, &'static str> {
    let bytes = std::fs::read(manifest).map_err(|_| "manifest is unreadable")?;
    if bytes.len() > 4096 {
        return Err("manifest exceeds its size bound");
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "manifest is not valid JSON")?;
    if value["schemaVersion"].as_u64() != Some(SCHEMA_VERSION)
        || value["adapter"] != ADAPTER
        || value["adapterVersion"].as_u64() != Some(ADAPTER_VERSION)
    {
        return Err("manifest does not describe this adapter generation");
    }
    if value["ompVersion"]
        .as_str()
        .is_none_or(|version| version.is_empty() || version.len() > 64)
    {
        return Err("manifest has no SDK version");
    }
    let root = manifest
        .parent()
        .ok_or("manifest has no installation root")?
        .to_path_buf();
    let bun = inside(&root, value["bunPath"].as_str())?;
    let entrypoint = inside(&root, value["entrypoint"].as_str())?;
    if !hello_patched(&root) {
        return Err("official SDK is not patched with the Ait bridge");
    }
    Ok(NativeHost { bun, entrypoint })
}

/// Resolve a manifest-relative path, refusing absolute paths and escapes from the root.
fn inside(root: &Path, value: Option<&str>) -> Result<PathBuf, &'static str> {
    let value = value.ok_or("manifest path is missing")?;
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("manifest path is malformed");
    }
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("manifest path escapes its installation");
    }
    let path = root.join(relative);
    let canonical = std::fs::canonicalize(&path).map_err(|_| "manifest path does not exist")?;
    let canonical_root = std::fs::canonicalize(root).map_err(|_| "installation root is missing")?;
    if !canonical.starts_with(&canonical_root) {
        return Err("manifest path resolves outside its installation");
    }
    Ok(canonical)
}

fn hello_patched(root: &Path) -> bool {
    std::fs::metadata(root.join(HELLO_MODULE)).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(test)]
mod tests;
