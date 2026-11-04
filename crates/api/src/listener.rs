use std::net::SocketAddr;

use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use tokio::net::TcpListener;

/// The accepted TCP socket's local address, used to validate wildcard listener requests.
///
/// Serve the API with `into_make_service_with_connect_info::<LocalAddress>()` so a
/// wildcard bind accepts only the Host authority of the actual destination interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalAddress(pub(super) SocketAddr, pub(super) SocketAddr);

impl Connected<IncomingStream<'_, TcpListener>> for LocalAddress {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        // This runs in the accept loop: a failed lookup must reject one request, not panic
        // the listener. The unspecified address admits no Host authority.
        let local = stream.io().local_addr().unwrap_or_else(|error| {
            tracing::warn!(%error, "accepted TCP socket has no local address");
            SocketAddr::from(([0, 0, 0, 0], 0))
        });
        Self(local, *stream.remote_addr())
    }
}

impl LocalAddress {
    pub(super) fn is_loopback_peer(self) -> bool {
        matches!(self.1.ip().to_canonical(),
            std::net::IpAddr::V4(ip) if ip == std::net::Ipv4Addr::LOCALHOST)
            || self.1.ip() == std::net::Ipv6Addr::LOCALHOST
    }

    /// Host authorities of the destination interface; none when it is unknown.
    pub(super) fn authorities(self) -> Vec<String> {
        if self.0.ip().is_unspecified() {
            return Vec::new();
        }
        let address = SocketAddr::new(self.0.ip().to_canonical(), self.0.port());
        allowed_authorities(address)
    }
}

pub(super) fn allowed_authorities(address: SocketAddr) -> Vec<String> {
    let mut authorities = vec![address.to_string()];
    if address.port() == 80 {
        authorities.push(address.to_string().trim_end_matches(":80").to_owned());
    }
    if address.ip().is_loopback() {
        authorities.push(format!("localhost:{}", address.port()));
        if address.port() == 80 {
            authorities.push("localhost".to_owned());
        }
    }
    authorities
}

#[cfg(test)]
mod tests;
