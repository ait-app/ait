//! Capability discovery and installation rules; request execution is owned by dispatchers.

use model::methods::MethodSpec;

/// Presence of independently composed services, supplied by the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "The host installs these independent services in every meaningful combination"
)]
pub struct InstalledServices {
    /// Push token store.
    pub push_tokens: bool,
    /// Directory service.
    pub directory: bool,
    /// Daemon service.
    pub daemon: bool,
    /// Workspace label service.
    pub workspace_labels: bool,
    /// Workspace automation service.
    pub workspace_automation: bool,
    /// Workspace state service.
    pub workspace_state: bool,
}

/// Return every component-owned method for negotiation and message validation.
/// # Returns
/// Method names and message directions, including events, without selecting a request handler.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    installed_methods(InstalledServices {
        push_tokens: true,
        directory: true,
        daemon: true,
        workspace_labels: true,
        workspace_automation: true,
        workspace_state: true,
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
        (true, crate::dispatch::BASE_METHODS),
        (services.push_tokens, crate::connection::push::METHODS),
        (true, crate::rpc::editor::METHODS),
        (true, crate::connection::session::METHODS),
        (true, crate::connection::creation::METHODS),
        (services.directory, crate::rpc::directory::METHODS),
        (
            services.directory,
            crate::rpc::directory::PROJECT_CONFIG_METHODS,
        ),
        (
            services.directory,
            crate::rpc::directory::PROJECT_ICON_METHODS,
        ),
        (services.daemon, crate::connection::daemon::METHODS),
        (
            services.workspace_labels,
            crate::rpc::workspace_labels::METHODS,
        ),
        (
            services.workspace_automation,
            crate::rpc::workspace_automation::METHODS,
        ),
        (
            services.workspace_state,
            crate::rpc::workspace_state::METHODS,
        ),
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
