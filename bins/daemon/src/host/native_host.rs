//! Ensure the OMP native host runtime is discoverable before OMP sessions are created.
//!
//! Two installation shapes are supported and neither blocks startup:
//!
//! * **Bundled** — a packaged desktop app or standalone daemon ships
//!   `provider-runtimes/omp-native-host/1` next to its binary; discovery finds it in place, so
//!   nothing is copied into user state.
//! * **Managed** — `<data-dir>/providers/omp-native-host/1` is produced by
//!   `runtimes/omp-native-host/install.mjs`. When that tree is absent and an installer is
//!   available (`AIT_SERVER_OMP_NATIVE_HOST_INSTALLER`, otherwise next to a development
//!   checkout's runtime sources), the daemon runs it once and reports the outcome.
//!
//! A missing runtime only degrades `/vibe` to an explicit refusal card; it never stops the
//! daemon.

use std::path::{Path, PathBuf};

use provider::native_host::discover;

/// Managed installation root inside the daemon data directory.
const MANAGED: &str = "providers/omp-native-host/1/manifest.json";
/// Installer entry point relative to the repository root in a development checkout.
const SOURCE_INSTALLER: &str = "runtimes/omp-native-host/install.mjs";

/// Resolve the runtime for `data_dir`, installing the managed copy when it is absent.
///
/// Returns the verified runtime, which the provider composition rediscovers on its own; this
/// probe exists so an operator sees the degradation in the daemon log instead of discovering it
/// only when `/vibe` is refused.
pub(crate) fn ensure_runtime(data_dir: &Path) {
    workspace(data_dir, &|name| std::env::var_os(name));
}

/// Probe with an explicit environment so startup and tests resolve identically.
pub(super) fn workspace(data_dir: &Path, environment: &dyn Fn(&str) -> Option<std::ffi::OsString>) {
    if let Some(host) = discover(data_dir, environment) {
        tracing::info!(
            entrypoint = %host.entrypoint().display(),
            "OMP native host runtime available"
        );
        return;
    }
    install(data_dir, environment);
    match discover(data_dir, environment) {
        Some(host) => tracing::info!(
            entrypoint = %host.entrypoint().display(),
            "OMP native host runtime installed"
        ),
        None => tracing::warn!(
            installed = %data_dir.join(MANAGED).display(),
            "OMP native host runtime is not installed; OMP will run the official binary and /vibe will be refused"
        ),
    }
}

/// Run the installer once when a managed tree is missing and an installer is reachable.
///
/// Any failure is a warning: the daemon keeps serving with the official OMP binary.
fn install(data_dir: &Path, environment: &dyn Fn(&str) -> Option<std::ffi::OsString>) {
    let managed = data_dir.join(MANAGED);
    if managed.exists() {
        // A manifest exists but did not verify; re-running the installer cannot decide which
        // tree wins, so the verified degradation is reported instead.
        tracing::warn!(
            manifest = %managed.display(),
            "Installed OMP native host runtime failed verification; not reinstalling"
        );
        return;
    }
    let Some(installer) = installer_path(environment) else {
        return;
    };
    let Some(node) = environment("AIT_SERVER_NODE_BIN").map(PathBuf::from) else {
        tracing::warn!(
            installer = %installer.display(),
            "OMP native host installer needs Node.js; set AIT_SERVER_NODE_BIN to install it"
        );
        return;
    };
    let result = std::process::Command::new(node)
        .arg(&installer)
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match result {
        Ok(output) if output.status.success() => {}
        Ok(output) => tracing::warn!(
            code = output.status.code(),
            detail = %String::from_utf8_lossy(&output.stderr).chars().take(512).collect::<String>(),
            "OMP native host installer failed; OMP keeps using the official binary"
        ),
        Err(error) => tracing::warn!(
            %error,
            "OMP native host installer could not run; OMP keeps using the official binary"
        ),
    }
}

/// Installer entry point, from the environment or the development checkout beside this binary.
fn installer_path(environment: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(explicit) = environment("AIT_SERVER_OMP_NATIVE_HOST_INSTALLER") {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    let executable = std::env::current_exe().ok()?;
    // target/debug/daemon -> repository root, where runtimes/ lives.
    let root = executable
        .parent()?
        .parent()?
        .parent()?
        .parent()
        .filter(|root| root.join("runtimes").is_dir())?;
    let installer = root.join(SOURCE_INSTALLER);
    installer.is_file().then_some(installer)
}

#[cfg(test)]
mod tests;
