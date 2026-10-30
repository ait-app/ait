//! Bounded diagnostic fields: never format peer-controlled errors or frame contents.
use tokio_tungstenite::tungstenite::{Error as SocketError, error::CapacityError};

pub(super) fn socket_error(operation: &'static str, error: &SocketError) {
    let kind = match error {
        SocketError::ConnectionClosed => "connection_closed",
        SocketError::AlreadyClosed => "already_closed",
        SocketError::Io(_) => "io",
        SocketError::Tls(_) => "tls",
        SocketError::Capacity(_) => "capacity",
        SocketError::Protocol(_) => "protocol",
        SocketError::WriteBufferFull(_) => "write_buffer_full",
        SocketError::Utf8(_) => "utf8",
        SocketError::AttackAttempt => "attack_attempt",
        SocketError::Url(_) => "url",
        SocketError::Http(_) => "http",
        SocketError::HttpFormat(_) => "http_format",
    };
    let os_error = match error {
        SocketError::Io(error) => error.raw_os_error(),
        _ => None,
    };
    // Sizes are local counters, not peer content; they show which limit an oversized message hit.
    let (size, max_size) = match error {
        SocketError::Capacity(CapacityError::MessageTooLong { size, max_size }) => {
            (Some(*size), Some(*max_size))
        }
        _ => (None, None),
    };
    tracing::warn!(
        operation,
        error_kind = kind,
        os_error,
        size,
        max_size,
        "Relay socket failed"
    );
}

#[cfg(test)]
pub(crate) mod tests;
