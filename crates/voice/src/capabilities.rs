//! Voice capability discovery and installation.

use model::methods::MethodSpec;

/// Return every component-owned method for negotiation and message validation.
/// # Returns
/// Method names and message directions, including events, without selecting a request handler.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    crate::connection::METHODS.iter().copied()
}

/// Return installed methods when this service is composed by the host.
/// # Arguments
/// * `installed` - Whether the host installed this service.
/// # Returns
/// Method names and message directions; an absent service advertises no capabilities.
pub fn installed_methods(installed: bool) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| installed)
}
