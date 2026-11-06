//! Default daemon data directory under the user's home, still honouring retired default locations.

use std::path::{Path, PathBuf};

/// Default directory names under the user's home, in lookup order: the current default, then the
/// retired Desktop-managed and standalone daemon defaults.
const DATA_DIRECTORIES: [&str; 3] = [".ait", ".ait-server-desktop", ".ait-server"];

/// Return the first existing default directory under `home`, or `<home>/.ait` when none exists.
/// Directories are never moved or renamed; the daemon creates a missing `.ait` on first use.
pub(super) fn resolve(home: &Path) -> PathBuf {
    DATA_DIRECTORIES
        .iter()
        .map(|name| home.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| home.join(DATA_DIRECTORIES[0]))
}

#[cfg(test)]
mod tests;
