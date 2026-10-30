use super::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    pub(crate) fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        let capture = self.clone();
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || capture.clone())
            .finish()
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

#[test]
fn errors_log_categories_without_peer_payloads() {
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), || {
        socket_error("read", &SocketError::Io(io::Error::other("private-token")));
        socket_error(
            "write",
            &SocketError::WriteBufferFull(Box::new(tokio_tungstenite::tungstenite::Message::Text(
                "private-payload".into(),
            ))),
        );
        socket_error("read", &SocketError::Io(io::Error::from_raw_os_error(104)));
    });
    let log = capture.text();
    assert!(log.contains("error_kind=\"io\""));
    assert!(log.contains("os_error=104"));
    assert!(log.contains("error_kind=\"write_buffer_full\""));
    assert!(!log.contains("private-"));
}

#[test]
fn socket_error_categories_never_include_sensitive_error_details() {
    use tokio_tungstenite::tungstenite::{
        error::{CapacityError, ProtocolError, TlsError, UrlError},
        http,
    };
    let capture = Capture::default();
    let cases = [
        (SocketError::ConnectionClosed, "connection_closed"),
        (SocketError::AlreadyClosed, "already_closed"),
        (SocketError::Tls(TlsError::InvalidDnsName), "tls"),
        (
            SocketError::Capacity(CapacityError::TooManyHeaders),
            "capacity",
        ),
        (
            SocketError::Protocol(ProtocolError::ResetWithoutClosingHandshake),
            "protocol",
        ),
        (SocketError::Utf8("private-detail".into()), "utf8"),
        (SocketError::AttackAttempt, "attack_attempt"),
        (SocketError::Url(UrlError::NoHostName), "url"),
        (
            SocketError::Http(Box::new(
                http::Response::builder()
                    .status(403)
                    .body(Some(b"private-detail".to_vec()))
                    .unwrap(),
            )),
            "http",
        ),
        (
            SocketError::HttpFormat(
                http::Request::builder()
                    .uri("invalid\nuri")
                    .body(())
                    .unwrap_err(),
            ),
            "http_format",
        ),
    ];
    for (error, expected) in cases {
        tracing::subscriber::with_default(capture.subscriber(), || socket_error("fixture", &error));
        assert!(
            capture
                .text()
                .contains(&format!("error_kind=\"{expected}\""))
        );
    }
    assert!(!capture.text().contains("private-detail"));
}
