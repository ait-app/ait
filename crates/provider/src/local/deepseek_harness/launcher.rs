//! Resolve CLI installations and desktop-bundled runtimes without opening the desktop UI.
use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::DeepSeekHarnessClient;
use crate::local::configuration::executable;

#[derive(Debug, Clone)]
pub(super) struct Desktop {
    pub(super) entrypoint: PathBuf,
    electron: bool,
}

impl DeepSeekHarnessClient {
    /// Prefer the installed CLI, falling back to the runtime bundled with the desktop app.
    /// Returns an unavailable `dsh` launcher when neither installation can be resolved.
    #[must_use]
    pub fn installed() -> Self {
        let paths: Vec<_> = std::env::var_os("PATH")
            .as_deref()
            .map(std::env::split_paths)
            .into_iter()
            .flatten()
            .filter(|path| path.is_absolute())
            .collect();
        let mut bundles = Vec::new();
        for path in &paths {
            if let Ok(program) = path.join("deepseek-harness").canonicalize()
                && let Some(parent) = program.parent()
            {
                bundles.push(parent.to_path_buf());
            }
        }
        if cfg!(target_os = "linux") {
            bundles
                .extend(["/opt/dsh-desktop-linux-bin", "/opt/deepseek-harness"].map(PathBuf::from));
        }
        if cfg!(target_os = "macos") {
            applications(Path::new("/Applications"), &mut bundles);
            if let Some(home) = std::env::var_os("HOME") {
                applications(&PathBuf::from(home).join("Applications"), &mut bundles);
            }
        }
        resolve(&paths, &bundles)
    }

    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.envs(self.environment.entries());
        if let Some(desktop) = &self.desktop {
            command.arg(&desktop.entrypoint);
            if desktop.electron {
                command.env("ELECTRON_RUN_AS_NODE", "1");
            }
        }
        command
    }
}

fn resolve(paths: &[PathBuf], bundles: &[PathBuf]) -> DeepSeekHarnessClient {
    let name = if cfg!(windows) { "dsh.exe" } else { "dsh" };
    if let Some(program) = paths
        .iter()
        .map(|path| path.join(name))
        .find(|path| executable(path))
    {
        return DeepSeekHarnessClient::new(program);
    }
    bundles
        .iter()
        .find_map(|bundle| desktop(bundle))
        .unwrap_or_else(|| DeepSeekHarnessClient::new("dsh".into()))
}

fn applications(directory: &Path, bundles: &mut Vec<PathBuf>) {
    if let Ok(entries) = directory.read_dir() {
        bundles.extend(
            entries
                .take(256)
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
                .map(|path| path.join("Contents")),
        );
    }
}

fn desktop(bundle: &Path) -> Option<DeepSeekHarnessClient> {
    let mac = bundle.file_name().is_some_and(|name| name == "Contents");
    let resources = bundle.join(if mac { "Resources" } else { "resources" });
    let package = resources.join("app/package.json");
    // Inspect only the known DSH desktop package, never an arbitrary Electron application.
    if package.metadata().ok()?.len() > 64 * 1024 {
        return None;
    }
    let identity: serde_json::Value = serde_json::from_slice(&std::fs::read(package).ok()?).ok()?;
    if identity["name"] != "@deepseek-ai/dsh-desktop" {
        return None;
    }
    let entrypoint = resources.join("app/dsh/node_modules/@deepseek-ai/dsh/lib/bin.js");
    if !entrypoint.is_file() {
        return None;
    }
    let node = resources.join("runtime/primary-runtime/dependencies/node/bin/node");
    let (program, electron) = if executable(&node) {
        (node, false)
    } else if mac {
        // Packaged macOS apps have one main executable; do not guess among several binaries.
        let mut executables = bundle
            .join("MacOS")
            .read_dir()
            .ok()?
            .take(16)
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| executable(path));
        let program = executables.next()?;
        if executables.next().is_some() {
            return None;
        }
        (program, true)
    } else {
        return None;
    };
    let mut client = DeepSeekHarnessClient::new(program);
    client.desktop = Some(Desktop {
        entrypoint,
        electron,
    });
    Some(client)
}

#[cfg(test)]
mod tests;
