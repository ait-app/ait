//! Crate-owned method metadata; the complete service is installed as one unit.

use model::methods::MethodSpec;

/// Return every method implemented by this crate for negotiation and envelope validation.
/// # Returns
/// Static names and message directions, independent of host installation or backend availability.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    [
        crate::connection::push::METHODS,
        crate::rpc::directory::METHODS,
        crate::rpc::directory::PROJECT_CONFIG_METHODS,
        crate::rpc::directory::PROJECT_ICON_METHODS,
        crate::connection::daemon::METHODS,
        crate::rpc::workspace_labels::METHODS,
        crate::rpc::workspace_automation::METHODS,
        crate::rpc::workspace_state::METHODS,
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
