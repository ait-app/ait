//! Host-facing filesystem dispatch failures shared by every capability group.

/// Safe host-facing dispatch failure; business failures remain in typed results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ErrorCode {
    /// Invalid parameters or binary frame.
    #[error("Invalid message or parameters")]
    InvalidMessage,
    /// Unknown filesystem method.
    #[error("Unknown method")]
    MethodNotFound,
    /// Filesystem I/O or result encoding failed.
    #[error("Project I/O failed")]
    ProjectIo,
    /// A connection-local resource limit was exceeded.
    #[error("Resource budget exhausted")]
    ResourceExhausted,
}

impl From<ErrorCode> for model::ErrorCode {
    fn from(error: ErrorCode) -> Self {
        match error {
            ErrorCode::InvalidMessage => Self::InvalidMessage,
            ErrorCode::MethodNotFound => Self::MethodNotFound,
            ErrorCode::ProjectIo => Self::ProjectIo,
            ErrorCode::ResourceExhausted => Self::ResourceExhausted,
        }
    }
}

impl From<model::ErrorCode> for ErrorCode {
    fn from(error: model::ErrorCode) -> Self {
        match error {
            model::ErrorCode::InvalidMessage => Self::InvalidMessage,
            model::ErrorCode::MethodNotFound => Self::MethodNotFound,
            model::ErrorCode::ResourceExhausted => Self::ResourceExhausted,
            // Codes owned by other capabilities collapse into project I/O failure.
            model::ErrorCode::BotNotFound
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
            | model::ErrorCode::UnsupportedCapability
            | model::ErrorCode::NotImplemented
            | model::ErrorCode::SubscriptionNotFound
            | model::ErrorCode::ServerDraining
            | model::ErrorCode::UnsupportedFormat
            | model::ErrorCode::IdempotencyConflict
            | model::ErrorCode::AgentNotFound
            | model::ErrorCode::AgentRevisionNotFound
            | model::ErrorCode::AgentRevisionConflict
            | model::ErrorCode::AgentDefaultConflict
            | model::ErrorCode::AgentDisabled
            | model::ErrorCode::AgentIsDefault
            | model::ErrorCode::CatalogBusy
            | model::ErrorCode::AgentIo
            | model::ErrorCode::WorkspaceNotFound
            | model::ErrorCode::RegistryIo
            | model::ErrorCode::DaemonConfigInvalid
            | model::ErrorCode::DaemonIo
            | model::ErrorCode::LabelNameEmpty
            | model::ErrorCode::LabelNotFound
            | model::ErrorCode::LabelNameTaken
            | model::ErrorCode::WorkspaceLabelStorageUncertain
            | model::ErrorCode::Forbidden
            | model::ErrorCode::ProjectIo => Self::ProjectIo,
        }
    }
}

#[cfg(test)]
mod tests;
