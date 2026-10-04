//! Child-process environment hygiene shared by every crate that starts programs.
//!
//! Server credentials live in the server's own environment and cannot be deleted from it (the
//! workspace forbids `unsafe`, and `std::env::remove_var` is unsafe in edition 2024). Every
//! spawn site therefore removes them from the child's inherited environment instead:
//!
//! ```
//! let mut command = std::process::Command::new("true");
//! for name in model::process::private_environment() {
//!     command.env_remove(name);
//! }
//! ```
//!
//! Remove inherited names before applying any environment the caller configures explicitly,
//! so explicit configuration keeps its meaning.

use std::ffi::{OsStr, OsString};

/// Prefixes of variables holding server credentials that no child process may inherit:
/// the Bonsai runtime token and the server's own authentication and credential variables.
pub const PRIVATE_ENVIRONMENT_PREFIXES: [&str; 2] = ["BONSAI_RUNTIME_", "AIT_SERVER_"];

/// Return whether a variable name carries a server credential.
///
/// Prefixes match regardless of ASCII case: Windows reads `bonsai_runtime_token` as
/// `BONSAI_RUNTIME_TOKEN`, so a case-sensitive match would let it through to children there.
#[must_use]
pub fn is_private(name: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    PRIVATE_ENVIRONMENT_PREFIXES.iter().any(|prefix| {
        name.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
    })
}

/// Return the names in `variables` that carry server credentials.
///
/// # Arguments
///
/// * `variables` - Name and value pairs, such as `std::env::vars_os()`.
///
/// # Returns
///
/// The private names, in input order.
#[must_use]
pub fn private_names<I>(variables: I) -> Vec<OsString>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    variables
        .into_iter()
        .filter_map(|(name, _)| is_private(&name).then_some(name))
        .collect()
}

/// Return the private variable names present in this process's environment; call
/// `env_remove` with each one before spawning a child.
#[must_use]
pub fn private_environment() -> Vec<OsString> {
    private_names(std::env::vars_os())
}

#[cfg(test)]
mod tests;
