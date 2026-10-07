//! Browser capability discovery and installation.

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

/// Return every implemented capability name, including events and client responses.
///
/// # Returns
/// Names derived from the component-owned method metadata.
pub fn implemented_capabilities() -> impl Iterator<Item = &'static str> {
    implemented_methods().map(|method| method.name)
}

/// Return capability names supported by the host's service installation.
///
/// # Arguments
/// * `installed` - Service presence supplied by the host.
///
/// # Returns
/// Names derived from installed component-owned method metadata.
pub fn installed_capabilities(installed: bool) -> impl Iterator<Item = &'static str> {
    installed_methods(installed).map(|method| method.name)
}
#[cfg(test)]
mod tests;
