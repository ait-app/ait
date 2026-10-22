//! WebSocket framing, authorization and bounded I/O for the hub adapter.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rustls::{ClientConfig, RootCertStore};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{
    Error as SocketError, Message, Utf8Bytes, client::IntoClientRequest, protocol::WebSocketConfig,
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};

use crate::{Error, diagnostics};

/// Connected hub or fixed local daemon WebSocket.
pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
/// Maximum size of one application frame on a data connection.
pub(super) const MAX_MESSAGE: usize = 1024 * 1024;

/// Shares trust configuration and the bounded TLS session cache across hub sockets.
#[derive(Clone, Debug)]
pub(super) struct Dialer {
    tls: Arc<ClientConfig>,
}

impl Default for Dialer {
    fn default() -> Self {
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("ring supports the default TLS protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
        Self {
            tls: Arc::new(config),
        }
    }
}

impl Dialer {
    /// Connect to the supplied fixed endpoint using a one-use ticket or local bearer token.
    ///
    /// # Errors
    /// Rejects invalid upgrade parameters, connection failures and a five-second setup timeout.
    pub(super) async fn connect(
        &self,
        url: &str,
        ticket: &str,
        max: usize,
    ) -> Result<Socket, Error> {
        let mut request = url.into_client_request().map_err(|_| Error::InvalidGrant)?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {ticket}")
                .parse()
                .map_err(|_| Error::InvalidGrant)?,
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(max))
            .max_frame_size(Some(max));
        timeout(
            Duration::from_secs(5),
            connect_async_tls_with_config(
                request,
                Some(config),
                true,
                Some(Connector::Rustls(self.tls.clone())),
            ),
        )
        .await
        .map_err(|_| {
            tracing::warn!(
                operation = "connect",
                reason = "timeout",
                timeout_ms = 5000,
                "Hub socket failed"
            );
            Error::Transport
        })?
        .map(|(socket, _)| socket)
        .map_err(|error| {
            diagnostics::socket_error("connect", &error);
            match error {
                SocketError::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    Error::Unauthorized
                }
                _ => Error::Transport,
            }
        })
    }
}

/// Send one frame, bounding blocked writes to five seconds.
///
/// # Errors
/// Returns a transport error if writing fails or times out.
pub(super) async fn send(socket: &mut Socket, message: Message) -> Result<(), Error> {
    timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| {
            tracing::warn!(
                operation = "send",
                reason = "timeout",
                timeout_ms = 5000,
                "Hub socket failed"
            );
            Error::Transport
        })?
        .map_err(|error| {
            diagnostics::socket_error("send", &error);
            Error::Transport
        })
}

/// Read a text frame while answering hop-local WebSocket pings.
///
/// # Errors
/// Rejects non-text application frames, disconnection and failed pong writes.
pub(super) async fn text(socket: &mut Socket) -> Result<Utf8Bytes, Error> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => return Ok(text),
            Some(Ok(Message::Ping(bytes))) => send(socket, Message::Pong(bytes)).await?,
            Some(Ok(Message::Pong(_))) => {}
            Some(Err(error)) => {
                diagnostics::socket_error("setup_read", &error);
                return Err(Error::Transport);
            }
            Some(Ok(Message::Close(frame))) => {
                tracing::info!(
                    close_code = frame.as_ref().map(|f| u16::from(f.code)),
                    reason = "close_frame",
                    "Hub setup read stopped"
                );
                return Err(Error::Transport);
            }
            other => {
                let reason = if other.is_none() {
                    "eof"
                } else {
                    "unexpected_frame"
                };
                tracing::warn!(reason, "Hub setup read stopped");
                return Err(Error::Transport);
            }
        }
    }
}
