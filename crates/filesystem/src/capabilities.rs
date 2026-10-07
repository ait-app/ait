//! Capability discovery and installation rules; request execution is owned by dispatchers.

/// Presence of independently composed services, supplied by the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "The host installs these independent services in every meaningful combination"
)]
pub struct InstalledServices {
    /// Skill installation service.
    pub skills: bool,
    /// Checkout service.
    pub checkout: bool,
    /// Forge service.
    pub forge: bool,
    /// File service.
    pub files: bool,
    /// GitHub projects service.
    pub github_projects: bool,
    /// Worktree service.
    pub worktrees: bool,
    /// Workspace recovery service.
    pub workspace_recovery: bool,
}

/// Return every implemented capability for negotiation and message validation.
/// # Returns
/// Static method names, including events, without selecting a request handler.
pub fn implemented_capabilities() -> impl Iterator<Item = &'static str> {
    installed_capabilities(InstalledServices {
        skills: true,
        checkout: true,
        forge: true,
        files: true,
        github_projects: true,
        worktrees: true,
        workspace_recovery: true,
    })
}

/// Return methods supported by `services`, using this crate's installation rules.
///
/// The iterator borrows static method names and excludes uninstalled optional services.
pub fn installed_capabilities(services: InstalledServices) -> impl Iterator<Item = &'static str> {
    [
        (services.skills, crate::service::skills::METHODS),
        (services.checkout, crate::dispatch::CHECKOUT_METHODS),
        (services.forge, crate::rpc::forge::METHODS),
        (services.files, crate::connection::files::METHODS),
        (
            services.github_projects,
            crate::rpc::github_projects::METHODS,
        ),
        (services.worktrees, crate::rpc::worktrees::METHODS),
        (
            services.workspace_recovery,
            crate::rpc::workspace_recovery::METHODS,
        ),
    ]
    .into_iter()
    .filter(|(installed, _)| *installed)
    .flat_map(|(_, methods)| methods.iter().copied())
}

#[cfg(test)]
mod tests;
