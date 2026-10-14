//! Bounded diagnostic fields: never format peer-controlled errors or frame contents.
use tokio_tungstenite::tungstenite::Error as SocketError;

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
    tracing::warn!(
        operation,
        error_kind = kind,
        os_error,
        "Relay socket failed"
    );
}

pub(super) fn setup_error(stage: &'static str, error: &crate::Error) {
    tracing::warn!(stage, %error, "Relay setup failed");
}

#[cfg(test)]
pub(crate) mod tests;
