//! Terminal method metadata and installation rules owned by this crate.

use model::methods::MethodSpec;

/// Return every component-owned method for negotiation and message validation.
/// # Returns
/// Method names and message directions, including events, without selecting a request handler.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    crate::connection::METHODS.iter().copied()
}

/// Presence of independently composed services, supplied by the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstalledServices {
    /// Terminal service.
    pub terminals: bool,
}

/// Return methods supported by `services`, using this crate's installation rules.
///
/// # Arguments
/// * `services` - Service presence supplied by the host.
///
/// # Returns
/// Static method metadata excluding uninstalled optional services.
pub fn installed_methods(services: InstalledServices) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| services.terminals)
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
/// * `services` - Service presence supplied by the host.
///
/// # Returns
/// Names derived from installed component-owned method metadata.
pub fn installed_capabilities(services: InstalledServices) -> impl Iterator<Item = &'static str> {
    installed_methods(services).map(|method| method.name)
}

#[cfg(test)]
mod tests;
