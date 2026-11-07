//! The `ait-hub-v1` serve link and the Hub downloads it carries, with a fixed
//! local destination. The legacy Relay control and data connections are gone:
//! this daemon only talks to centers whose Hub holds the node session lease and
//! carries downloads (`hub_features`), and those centers bridge downloads of
//! older clients onto the Hub link.
mod diagnostics;
mod download;
pub mod hub;
mod protocol;
pub mod rpc;
mod transport;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use url::Url;

pub use protocol::Status;
use transport::Dialer;

/// Owns the Hub serve link and its local destination.
#[derive(Clone, Debug)]
pub struct Connector {
    local: Arc<Local>,
    shutdown: CancellationToken,
    hub: hub::Connector,
}

#[derive(Debug)]
struct Local {
    dialer: Dialer,
    url: String,
    server_id: String,
    instance_id: String,
    /// Local download client, built on first use and reused for every later download.
    downloads: OnceCell<reqwest::Client>,
    /// In-process API serving Hub business sessions, attached once it can serve.
    api: std::sync::OnceLock<Arc<dyn hub::bridge::LocalApi>>,
}

/// Connector failures deliberately omit URLs and credentials.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Only explicit HTTPS or loopback HTTP centers are accepted.
    #[error("invalid relay center or ticket")]
    InvalidGrant,
    /// An upgrade or write failed or exceeded its deadline.
    #[error("relay transport unavailable")]
    Transport,
    /// The center did not follow the agreed protocol, or lacks a Hub feature
    /// this daemon requires.
    #[error("relay protocol rejected")]
    Protocol,
}

impl Connector {
    /// Bind to this process's actual local address and identity.
    #[must_use]
    pub fn new(address: SocketAddr, server_id: String, instance_id: String) -> Self {
        let address = match address {
            SocketAddr::V4(mut address) => {
                if address.ip().is_unspecified() {
                    address.set_ip(std::net::Ipv4Addr::LOCALHOST);
                }
                SocketAddr::V4(address)
            }
            SocketAddr::V6(mut address) => {
                if address.ip().is_unspecified() {
                    address.set_ip(std::net::Ipv6Addr::LOCALHOST);
                }
                SocketAddr::V6(address)
            }
        };
        let shutdown = CancellationToken::new();
        let local = Arc::new(Local {
            dialer: Dialer::default(),
            url: format!("ws://{address}/v1/ws"),
            server_id,
            instance_id,
            downloads: OnceCell::new(),
            api: std::sync::OnceLock::new(),
        });
        Self {
            hub: hub::Connector::new(local.clone(), shutdown.clone()),
            shutdown,
            local,
        }
    }

    /// Serve Hub business sessions with `api`; the first attachment wins. Until
    /// one is attached, sessions are refused as `unavailable`.
    pub fn attach_api(&self, api: Arc<dyn hub::bridge::LocalApi>) {
        let _ = self.local.api.set(api);
    }

    /// The hub serve link sharing this connector's local destination and shutdown.
    #[must_use]
    pub fn hub(&self) -> &hub::Connector {
        &self.hub
    }

    /// Permanently close connector admission when the local runtime starts draining.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }
}

fn center_url(value: &str) -> Result<Url, Error> {
    let mut url = Url::parse(value).map_err(|_| Error::InvalidGrant)?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && local)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidGrant);
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

/// Reject anything but a one-use 64-hex center ticket.
fn validate_ticket(ticket: &str) -> Result<(), Error> {
    if ticket.len() != 64 || !ticket.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidGrant);
    }
    Ok(())
}

fn endpoint(center: &Url, path: &str) -> String {
    let mut url = center.clone();
    url.set_path(&format!("{}{path}", center.path()));
    let scheme = if center.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    let _ = url.set_scheme(scheme);
    url.to_string()
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
