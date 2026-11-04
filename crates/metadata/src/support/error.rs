//! Safe metadata business failures mapped into the host RPC error envelope.

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

impl From<model::ErrorCode> for ErrorCode {
    fn from(error: model::ErrorCode) -> Self {
        match error {
            model::ErrorCode::IdempotencyConflict => Self::IdempotencyConflict,
            model::ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            model::ErrorCode::InvalidMessage => Self::InvalidMessage,
            model::ErrorCode::UnsupportedCapability => Self::UnsupportedCapability,
            model::ErrorCode::MethodNotFound => Self::MethodNotFound,
            model::ErrorCode::DaemonConfigInvalid => Self::DaemonConfigInvalid,
            model::ErrorCode::DaemonIo => Self::DaemonIo,
            model::ErrorCode::WorkspaceNotFound => Self::WorkspaceNotFound,
            model::ErrorCode::LabelNameEmpty => Self::LabelNameEmpty,
            model::ErrorCode::LabelNotFound => Self::LabelNotFound,
            model::ErrorCode::LabelNameTaken => Self::LabelNameTaken,
            model::ErrorCode::WorkspaceLabelStorageUncertain => {
                Self::WorkspaceLabelStorageUncertain
            }
            // Codes owned by other capabilities collapse into the retryable registry failure.
            model::ErrorCode::RegistryIo
            | model::ErrorCode::BotNotFound
            | model::ErrorCode::BotConflict
            | model::ErrorCode::BotIo
            | model::ErrorCode::RelayManaged
            | model::ErrorCode::ScheduleRequestFailed
            | model::ErrorCode::ScheduleRunning
            | model::ErrorCode::SpeechIo
            | model::ErrorCode::TerminalIo
            | model::ErrorCode::TerminalNotFound
            | model::ErrorCode::WorkingDirectoryUnavailable
            | model::ErrorCode::IncompatibleVersion
            | model::ErrorCode::NotImplemented
            | model::ErrorCode::SubscriptionNotFound
            | model::ErrorCode::ServerDraining
            | model::ErrorCode::UnsupportedFormat
            | model::ErrorCode::ProjectIo
            | model::ErrorCode::AgentNotFound
            | model::ErrorCode::AgentRevisionNotFound
            | model::ErrorCode::AgentRevisionConflict
            | model::ErrorCode::AgentDefaultConflict
            | model::ErrorCode::AgentDisabled
            | model::ErrorCode::AgentIsDefault
            | model::ErrorCode::CatalogBusy
            | model::ErrorCode::AgentIo
            | model::ErrorCode::Forbidden => Self::RegistryIo,
        }
    }
}

#[cfg(test)]
mod tests;
