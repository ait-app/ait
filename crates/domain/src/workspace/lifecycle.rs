//! Workspace registration parameters and safe collaboration failures.

/// Parameters for registering a new Workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceCreation<'a> {
    /// Existing directory to inspect.
    pub path: &'a str,
    /// Optional user title.
    pub title: Option<String>,
    /// Explicit active owning Project, or automatic registration.
    pub project_id: Option<&'a str>,
    /// Caller-reserved identity, or a freshly generated identity.
    pub workspace_id: Option<String>,
    /// Whether a first Agent will follow creation.
    pub expects_initial_agent: bool,
    /// Creation and update timestamp.
    pub timestamp: &'a str,
}

/// Safe failure returned by a Workspace collaboration adapter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WorkspaceLifecycleError {
    /// Business error description without native diagnostics or credentials.
    pub message: String,
}
