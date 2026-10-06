//! Terminal method groups and installation rules owned by this crate.

/// Return every implemented capability for negotiation and message validation.
/// # Returns
/// Static method names, including events, without selecting a request handler.
pub fn implemented_capabilities() -> impl Iterator<Item = &'static str> {
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
/// The iterator borrows static method names and excludes uninstalled optional services.
pub fn installed_capabilities(services: InstalledServices) -> impl Iterator<Item = &'static str> {
    implemented_capabilities().filter(move |_| services.terminals)
}

#[cfg(test)]
mod tests;
