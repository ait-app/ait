//! Capability discovery and installation rules; request execution is owned by dispatchers.

/// Presence of independently composed services, supplied by the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstalledServices {
    /// Versioned Agent preset service.
    pub agents: bool,
    /// Agent runtime directory service.
    pub agent_runtime: bool,
    /// Native Agent execution service, which also implements runtime methods.
    pub agent_execution: bool,
}

/// Return every implemented capability for negotiation and message validation.
/// # Returns
/// Static method names, including events, without selecting a request handler.
pub fn implemented_capabilities() -> impl Iterator<Item = &'static str> {
    installed_capabilities(InstalledServices {
        agents: true,
        agent_runtime: true,
        agent_execution: true,
    })
}

/// Return methods supported by `services`, using this crate's installation rules.
///
/// The iterator borrows static method names and excludes uninstalled optional services.
pub fn installed_capabilities(services: InstalledServices) -> impl Iterator<Item = &'static str> {
    [
        (services.agents, crate::rpc::agents::METHODS),
        (
            services.agent_runtime || services.agent_execution,
            crate::rpc::agent_runtime::METHODS,
        ),
        (
            services.agent_execution,
            crate::dispatch::agent_execution::METHODS,
        ),
        (services.agent_execution, crate::rpc::timeline::METHODS),
        (services.agent_execution, crate::dispatch::CATALOG_METHODS),
    ]
    .into_iter()
    .filter(|(installed, _)| *installed)
    .flat_map(|(_, methods)| methods.iter().copied())
}

#[cfg(test)]
mod tests;
