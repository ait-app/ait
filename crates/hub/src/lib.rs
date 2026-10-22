//! Host serve endpoint for ait-hub-v1.

mod diagnostics;
mod driver;
mod link;
pub mod rpc;
mod session;
mod transport;
mod wire;

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Ephemeral serve authority from the account or machine credential owner.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlGrant {
    /// HTTPS authority, including any gateway path prefix.
    pub center_url: String,
    /// One-use Hub ticket; never a user JWT or local API token.
    pub hub_ticket: SecretString,
    /// Existing leased node activation for this daemon.
    pub node_session_id: Uuid,
}

/// Non-secret Hub state. Recovering links keep their local sessions alive.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// True after a valid welcome on the current physical connection.
    pub online: bool,
    /// Includes initial connection and bounded resume attempts.
    pub connecting: bool,
    /// Logical link identity; no resume credential is exposed.
    pub link: Option<Uuid>,
    /// Sanitized failure category, never an upstream error body.
    pub error: Option<&'static str>,
}

#[derive(Debug)]
struct State {
    status: Status,
    generation: Uuid,
    cancel: CancellationToken,
}

/// Owns one serve link and its local sockets.
#[derive(Clone, Debug)]
pub struct Connector {
    local: Arc<Local>,
    state: Arc<Mutex<State>>,
    shutdown: CancellationToken,
}

impl Connector {
    /// Bind the Hub connector to this daemon's actual local API.
    ///
    /// # Arguments
    /// * address - Bound API socket; wildcard addresses are converted to loopback.
    /// * token - Local API credential, never forwarded to the Hub.
    /// * `server_id` - Stable identity reported in local control responses.
    /// * `instance_id` - Identity of this daemon process.
    #[must_use]
    pub fn new(
        address: std::net::SocketAddr,
        token: SecretString,
        server_id: String,
        instance_id: String,
    ) -> Self {
        let address = if address.ip().is_unspecified() {
            std::net::SocketAddr::new(
                if address.is_ipv4() {
                    std::net::Ipv4Addr::LOCALHOST.into()
                } else {
                    std::net::Ipv6Addr::LOCALHOST.into()
                },
                address.port(),
            )
        } else {
            address
        };
        let local = Arc::new(Local {
            visits: Arc::new(tokio::sync::Semaphore::new(16)),
            dialer: transport::Dialer::default(),
            url: format!("ws://{address}/v1/ws"),
            token,
            server_id,
            instance_id,
        });
        Self {
            local,
            state: Arc::new(Mutex::new(State {
                status: Status::default(),
                generation: Uuid::new_v4(),
                cancel: CancellationToken::new(),
            })),
            shutdown: CancellationToken::new(),
        }
    }

    /// Start a fresh serve link, replacing the previous link and its sessions.
    ///
    /// # Arguments
    /// * grant - Serve ticket from the trusted account owner for a leased node.
    /// # Errors
    /// Rejects malformed authority/tickets or admission after process shutdown.
    pub async fn start(&self, grant: ControlGrant) -> Result<(), Error> {
        let center = center_url(&grant.center_url)?;
        if grant.node_session_id.is_nil() || !valid_ticket(grant.hub_ticket.expose_secret()) {
            return Err(Error::InvalidGrant);
        }
        let generation = Uuid::new_v4();
        let cancel = self.shutdown.child_token();
        {
            let mut state = self.state.lock().await;
            if self.shutdown.is_cancelled() {
                return Err(Error::Transport);
            }
            state.cancel.cancel();
            state.cancel = cancel.clone();
            state.generation = generation;
            state.status = Status {
                connecting: true,
                ..Status::default()
            };
        }
        let connector = self.clone();
        tokio::spawn(async move {
            let mut driver = driver::Driver::new(connector.local.clone());
            let hub_url = endpoint(&center, "v1/hub");
            let result = tokio::select! {
                biased;
                () = cancel.cancelled() => Ok(()),
                result = driver.run(&connector, generation, &hub_url, grant.hub_ticket) => result,
            };
            drop(driver);
            if let Err(reason) = &result {
                tracing::warn!(reason, "Hub serve link stopped");
            }
            let mut state = connector.state.lock().await;
            if state.generation == generation {
                state.status.online = false;
                state.status.connecting = false;
                state.status.error = result.err();
            }
        });
        Ok(())
    }

    /// Read public state without exposing tickets or resume credentials.
    pub async fn status(&self) -> Status {
        self.state.lock().await.status.clone()
    }

    /// Cancel the link and all local sessions, including during reconnects.
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.cancel.cancel();
        state.generation = Uuid::new_v4();
        state.status = Status::default();
    }

    /// Permanently reject new starts and cancel the link during process drain.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    async fn connected(&self, generation: Uuid, online: bool, link: Option<Uuid>) {
        let mut state = self.state.lock().await;
        if state.generation == generation {
            state.status = Status {
                online,
                connecting: !online,
                link,
                error: None,
            };
        }
    }
}

fn valid_ticket(ticket: &str) -> bool {
    ticket.len() == 64 && ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;

#[derive(Debug)]
struct Local {
    visits: Arc<tokio::sync::Semaphore>,
    dialer: transport::Dialer,
    url: String,
    token: SecretString,
    server_id: String,
    instance_id: String,
}

/// Bounded and credential-free transport failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid authority or one-use ticket.
    #[error("invalid hub center or ticket")]
    InvalidGrant,
    /// Connection, deadline or bounded queue failure.
    #[error("hub transport unavailable")]
    Transport,
    /// Invalid Hub or local daemon frame.
    #[error("hub protocol rejected")]
    Protocol,
    /// Reconnect credential no longer identifies an authorized link.
    #[error("hub credential rejected")]
    Unauthorized,
}

fn center_url(value: &str) -> Result<url::Url, Error> {
    let mut url = url::Url::parse(value).map_err(|_| Error::InvalidGrant)?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && local)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(Error::InvalidGrant);
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn endpoint(center: &url::Url, path: &str) -> String {
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
