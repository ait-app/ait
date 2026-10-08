//! Transport-independent Agent requests and failures.

pub mod agent_execution;
pub mod agent_runtime;
pub mod agents;

/// Stable Agent failures mapped by the transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ErrorCode {
    /// A request exceeded a budget or conflicted with an immutable receipt.
    #[error("ResourceExhausted")]
    ResourceExhausted,
    /// Invalid Agent request.
    #[error("InvalidMessage")]
    InvalidMessage,
    /// The requested working directory cannot be opened.
    #[error("WorkingDirectoryUnavailable")]
    WorkingDirectoryUnavailable,
    /// Requested runtime capability is unavailable.
    #[error("UnsupportedCapability")]
    UnsupportedCapability,
    /// Unknown Agent method.
    #[error("MethodNotFound")]
    MethodNotFound,
    /// Agent persistence failed.
    #[error("AgentIo")]
    AgentIo,
    /// No Agent has this identity.
    #[error("AgentNotFound")]
    AgentNotFound,
    /// No such immutable revision.
    #[error("AgentRevisionNotFound")]
    AgentRevisionNotFound,
    /// The observed Agent revision has changed.
    #[error("AgentRevisionConflict")]
    AgentRevisionConflict,
    /// The default selection has changed.
    #[error("AgentDefaultConflict")]
    AgentDefaultConflict,
    /// The selected Agent is disabled.
    #[error("AgentDisabled")]
    AgentDisabled,
    /// The Agent is still the default.
    #[error("AgentIsDefault")]
    AgentIsDefault,
    /// The retry key was reused with different arguments.
    #[error("IdempotencyConflict")]
    IdempotencyConflict,
    /// The catalog is busy.
    #[error("CatalogBusy")]
    CatalogBusy,
    /// The persisted format is unsupported.
    #[error("UnsupportedFormat")]
    UnsupportedFormat,
    /// Workspace placement could not be read.
    #[error("RegistryIo")]
    RegistryIo,
}

impl From<ErrorCode> for model::ErrorCode {
    fn from(error: ErrorCode) -> Self {
        match error {
            ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            ErrorCode::InvalidMessage => Self::InvalidMessage,
            ErrorCode::WorkingDirectoryUnavailable => Self::WorkingDirectoryUnavailable,
            ErrorCode::UnsupportedCapability => Self::UnsupportedCapability,
            ErrorCode::MethodNotFound => Self::MethodNotFound,
            ErrorCode::AgentIo => Self::AgentIo,
            ErrorCode::AgentNotFound => Self::AgentNotFound,
            ErrorCode::AgentRevisionNotFound => Self::AgentRevisionNotFound,
            ErrorCode::AgentRevisionConflict => Self::AgentRevisionConflict,
            ErrorCode::AgentDefaultConflict => Self::AgentDefaultConflict,
            ErrorCode::AgentDisabled => Self::AgentDisabled,
            ErrorCode::AgentIsDefault => Self::AgentIsDefault,
            ErrorCode::IdempotencyConflict => Self::IdempotencyConflict,
            ErrorCode::CatalogBusy => Self::CatalogBusy,
            ErrorCode::UnsupportedFormat => Self::UnsupportedFormat,
            ErrorCode::RegistryIo => Self::RegistryIo,
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod fork_context;
pub(crate) mod timeline;

impl From<model::ErrorCode> for ErrorCode {
    fn from(error: model::ErrorCode) -> Self {
        match error {
            model::ErrorCode::InvalidMessage => Self::InvalidMessage,
            model::ErrorCode::WorkingDirectoryUnavailable => Self::WorkingDirectoryUnavailable,
            model::ErrorCode::UnsupportedCapability => Self::UnsupportedCapability,
            model::ErrorCode::MethodNotFound => Self::MethodNotFound,
            model::ErrorCode::RegistryIo => Self::RegistryIo,
            model::ErrorCode::IdempotencyConflict => Self::IdempotencyConflict,
            model::ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            model::ErrorCode::AgentNotFound => Self::AgentNotFound,
            model::ErrorCode::UnsupportedFormat => Self::UnsupportedFormat,
            model::ErrorCode::CatalogBusy => Self::CatalogBusy,
            _ => Self::AgentIo,
        }
    }
}
