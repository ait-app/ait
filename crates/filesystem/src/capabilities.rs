//! Crate-owned method metadata; the complete service is installed as one unit.

use model::methods::MethodSpec;

/// Return every method implemented by this crate for negotiation and envelope validation.
/// # Returns
/// Static names and message directions, independent of host installation or backend availability.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    [
        crate::service::skills::METHODS,
        crate::dispatch::CHECKOUT_METHODS,
        crate::rpc::forge::METHODS,
        crate::connection::files::METHODS,
        crate::rpc::github_projects::METHODS,
        crate::rpc::worktrees::METHODS,
        crate::rpc::workspace_recovery::METHODS,
    ]
    .into_iter()
    .flat_map(|methods| methods.iter().copied())
}

/// Return this crate's complete method set when its service is `installed`.
/// # Returns
/// Every implemented method for a present service, or an empty iterator for an absent service.
pub fn installed_methods(installed: bool) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| installed)
}

#[cfg(test)]
mod tests;
