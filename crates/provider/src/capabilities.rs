//! Capability discovery and installation rules; request execution is owned by dispatchers.

use model::methods::MethodSpec;

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

/// Return every component-owned method for negotiation and message validation.
/// # Returns
/// Method names and message directions, including events, without selecting a request handler.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    installed_methods(InstalledServices {
        agents: true,
        agent_runtime: true,
        agent_execution: true,
    })
}

/// Return methods supported by `services`, using this crate's installation rules.
///
/// # Arguments
/// * `services` - Service presence supplied by the host.
///
/// # Returns
/// Static method metadata excluding uninstalled optional services.
pub fn installed_methods(services: InstalledServices) -> impl Iterator<Item = MethodSpec> {
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
