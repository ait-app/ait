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
