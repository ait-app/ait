//! File-backed Paseo Project/Workspace registries; hosts must hold a data-directory lease.
//! Rust translation and modifications: see third-party/paseo/NOTICE and LICENSE.

mod listeners;
mod paths;
mod projects;
mod workspaces;

pub use projects::FileBackedProjectRegistry;
pub use workspaces::FileBackedWorkspaceRegistry;

use domain::workspace::registry::RegistryError;

impl From<crate::registry::Error> for RegistryError {
    fn from(error: crate::registry::Error) -> Self {
        match error {
            crate::registry::Error::InvalidRecord => Self::InvalidRecord,
            crate::registry::Error::InvalidFile => Self::InvalidFile,
            crate::registry::Error::Io => Self::Io,
            crate::registry::Error::Frozen => Self::Frozen,
        }
    }
}

fn generate_id(prefix: &str) -> Result<String, RegistryError> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|_| RegistryError::Io)?;
    Ok(format!("{prefix}{:016x}", u64::from_be_bytes(bytes)))
}

#[cfg(test)]
mod tests;
