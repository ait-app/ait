//! Voice capability discovery and installation.

/// Return every implemented capability for negotiation and message validation.
/// # Returns
/// Static method names, including events, without selecting a request handler.
pub fn implemented_capabilities() -> impl Iterator<Item = &'static str> {
    crate::connection::METHODS.iter().copied()
}

/// Return installed methods when this service is composed by the host.
/// # Arguments
/// * `installed` - Whether the host installed this service.
/// # Returns
/// Static method names; an absent service advertises no capabilities.
pub fn installed_capabilities(installed: bool) -> impl Iterator<Item = &'static str> {
    implemented_capabilities().filter(move |_| installed)
}
