use std::ffi::OsStr;
use std::path::PathBuf;

/// Resolve the installed Cursor CLI from process PATH and user home; return a fallback if absent.
pub(super) fn installed_program() -> PathBuf {
    resolve(
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve(path: Option<&OsStr>, home: Option<PathBuf>) -> PathBuf {
    let directories = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|directory| directory.is_absolute())
        .chain(home.into_iter().map(|home| home.join(".local/bin")));
    directories
        .flat_map(|directory| [directory.join("cursor-agent"), directory.join("agent")])
        .find(|program| crate::local::configuration::executable(program))
        .unwrap_or_else(|| PathBuf::from("cursor-agent"))
}

#[cfg(test)]
mod tests;
