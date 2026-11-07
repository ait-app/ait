//! WebSocket dialing and authorization for hub links.

use std::sync::Arc;
use std::time::Duration;

use rustls::{ClientConfig, RootCertStore};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};

use crate::{Error, diagnostics};

/// Connected hub WebSocket.
pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Why an upgrade did not produce a socket.
enum Dial {
    /// The URL or credential could not form a request.
    Request,
    /// The server answered the upgrade with 401 or 403.
    Refused,
    /// Network, TLS, timeout or any other upgrade failure.
    Failed,
}

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
    /// Connect to the supplied fixed endpoint with a one-use ticket or resume
    /// token. An HTTP 401/403 upgrade refusal is reported as `InvalidGrant`, so a
    /// refused credential is not retried.
    ///
    /// # Errors
    /// `InvalidGrant` for an unusable request or a refused credential, and
    /// `Error::Transport` otherwise.
    pub(super) async fn connect(
        &self,
        url: &str,
        credential: &str,
        max: usize,
    ) -> Result<Socket, Error> {
        self.dial(url, credential, max)
            .await
            .map_err(|error| match error {
                Dial::Request | Dial::Refused => Error::InvalidGrant,
                Dial::Failed => Error::Transport,
            })
    }

    async fn dial(&self, url: &str, ticket: &str, max: usize) -> Result<Socket, Dial> {
        let mut request = url.into_client_request().map_err(|_| Dial::Request)?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {ticket}")
                .parse()
                .map_err(|_| Dial::Request)?,
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
                "Relay socket failed"
            );
            Dial::Failed
        })?
        .map(|(socket, _)| socket)
        .map_err(|error| {
            diagnostics::socket_error("connect", &error);
            match &error {
                tokio_tungstenite::tungstenite::Error::Http(response)
                    if matches!(response.status().as_u16(), 401 | 403) =>
                {
                    Dial::Refused
                }
                _ => Dial::Failed,
            }
        })
    }
}

#[cfg(test)]
mod tests;
