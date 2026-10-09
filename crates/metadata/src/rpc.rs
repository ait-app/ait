//! Transport-independent metadata request handling and safe business failures.

pub(crate) mod daemon;
pub mod directory;
/// Legacy desktop editor compatibility responses.
pub(crate) mod editor;
pub mod server;
pub(crate) mod workspace_labels;

/// Stable metadata failure mapped into the host's public RPC error envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ErrorCode {
    /// A request exceeded a budget or conflicted with an immutable receipt.
    #[error("IdempotencyConflict")]
    IdempotencyConflict,
    /// A request exceeded a budget or conflicted with an immutable receipt.
    #[error("ResourceExhausted")]
    ResourceExhausted,
    /// Metadata invalid message failure.
    #[error("InvalidMessage")]
    InvalidMessage,
    /// Metadata unsupported capability failure.
    #[error("UnsupportedCapability")]
    UnsupportedCapability,
    /// Metadata method not found failure.
    #[error("MethodNotFound")]
    MethodNotFound,
    /// Metadata registry io failure.
    #[error("RegistryIo")]
    RegistryIo,
    /// Metadata daemon config invalid failure.
    #[error("DaemonConfigInvalid")]
    DaemonConfigInvalid,
    /// Metadata daemon io failure.
    #[error("DaemonIo")]
    DaemonIo,
    /// Metadata workspace not found failure.
    #[error("WorkspaceNotFound")]
    WorkspaceNotFound,
    /// Metadata label name empty failure.
    #[error("LabelNameEmpty")]
    LabelNameEmpty,
    /// Metadata label not found failure.
    #[error("LabelNotFound")]
    LabelNotFound,
    /// Metadata label name taken failure.
    #[error("LabelNameTaken")]
    LabelNameTaken,
    /// Metadata workspace label storage uncertain failure.
    #[error("WorkspaceLabelStorageUncertain")]
    WorkspaceLabelStorageUncertain,
}
pub mod workspace_automation;
pub mod workspace_state;

impl From<ErrorCode> for model::ErrorCode {
    fn from(error: ErrorCode) -> Self {
        match error {
            ErrorCode::IdempotencyConflict => Self::IdempotencyConflict,
            ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            ErrorCode::InvalidMessage => Self::InvalidMessage,
            ErrorCode::UnsupportedCapability => Self::UnsupportedCapability,
            ErrorCode::MethodNotFound => Self::MethodNotFound,
            ErrorCode::RegistryIo => Self::RegistryIo,
            ErrorCode::DaemonConfigInvalid => Self::DaemonConfigInvalid,
            ErrorCode::DaemonIo => Self::DaemonIo,
            ErrorCode::WorkspaceNotFound => Self::WorkspaceNotFound,
            ErrorCode::LabelNameEmpty => Self::LabelNameEmpty,
            ErrorCode::LabelNotFound => Self::LabelNotFound,
            ErrorCode::LabelNameTaken => Self::LabelNameTaken,
            ErrorCode::WorkspaceLabelStorageUncertain => Self::WorkspaceLabelStorageUncertain,
        }
    }
}

#[cfg(test)]
mod tests;

impl From<model::ErrorCode> for ErrorCode {
    fn from(error: model::ErrorCode) -> Self {
        match error {
            model::ErrorCode::InvalidMessage => Self::InvalidMessage,
            model::ErrorCode::UnsupportedCapability => Self::UnsupportedCapability,
            model::ErrorCode::MethodNotFound => Self::MethodNotFound,
            model::ErrorCode::IdempotencyConflict => Self::IdempotencyConflict,
            model::ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            _ => Self::RegistryIo,
        }
    }
}
