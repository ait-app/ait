//! Outbound control and reverse data transport. Local destinations are fixed at construction.
mod bridge;
mod diagnostics;
mod download;
mod protocol;
pub mod rpc;
mod transport;
mod tunnel;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use secrecy::SecretString;
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use url::Url;
use uuid::Uuid;

use protocol::{ControlCommand, ControlHello, ControlWelcome, DataMode};
pub use protocol::{ControlGrant, Status};
use transport::{Dialer, Socket, receive, send};

#[derive(Debug)]
struct State {
    status: Status,
    generation: Uuid,
    cancel: CancellationToken,
}

/// Owns at most one outbound control connection and its bounded independent data tasks.
#[derive(Clone, Debug)]
pub struct Connector {
    state: Arc<Mutex<State>>,
    local: Arc<Local>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
struct Local {
    visits: Arc<Semaphore>,
    dialer: Dialer,
    url: String,
    token: SecretString,
    server_id: String,
    instance_id: String,
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
    /// The center did not follow the agreed control or pairing protocol.
    #[error("relay protocol rejected")]
    Protocol,
}

impl Connector {
    /// Bind to this process's actual local address and credentials.
    #[must_use]
    pub fn new(
        address: SocketAddr,
        token: SecretString,
        server_id: String,
        instance_id: String,
    ) -> Self {
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
        Self {
            shutdown: CancellationToken::new(),
            state: Arc::new(Mutex::new(State {
                status: Status::default(),
                generation: Uuid::new_v4(),
                cancel: CancellationToken::new(),
            })),
            local: Arc::new(Local {
                visits: Arc::new(Semaphore::new(16)),
                dialer: Dialer::default(),
                url: format!("ws://{address}/v1/ws"),
                token,
                server_id,
                instance_id,
            }),
        }
    }

    /// Start a new control attempt, cancelling all data work from the previous attempt.
    ///
    /// # Errors
    /// Rejects non-TLS remote centers, malformed URLs and invalid one-use tickets.
    pub async fn start(&self, grant: ControlGrant) -> Result<(), Error> {
        if self.shutdown.is_cancelled() {
            return Err(Error::Transport);
        }
        let center = center_url(&grant.center_url)?;
        if grant.control_ticket.len() != 64
            || !grant.control_ticket.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::InvalidGrant);
        }
        let generation = Uuid::new_v4();
        let cancel = self.shutdown.child_token();
        let resume = {
            let mut state = self.state.lock().await;
            state.cancel.cancel();
            state.cancel = cancel.clone();
            state.generation = generation;
            state.status.connecting = true;
            state.status.online = false;
            state.status.error = None;
            state.status.epoch
        };
        let connector = self.clone();
        let span = tracing::info_span!("relay_control", %generation, node_session_id = %grant.node_session_id);
        tokio::spawn(async move {
            let result = tokio::select! {
                () = cancel.cancelled() => {
                    tracing::info!(%generation, reason = "cancelled", "Relay control stopped");
                    Ok(())
                },
                result = connector.run(center, grant, generation, resume) => {
                    if let Err(error) = &result {
                        tracing::warn!(%generation, %error, "Relay control failed; active data sessions will close");
                    }
                    result
                },
            };
            cancel.cancel();
            let mut state = connector.state.lock().await;
            if state.generation == generation {
                state.status.connecting = false;
                state.status.online = false;
                state.status.error = result.err().map(|error| error.to_string());
            }
        }.instrument(span));
        Ok(())
    }

    /// Permanently close connector admission when the local runtime starts draining.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Stop all control/data activity immediately; safe to call repeatedly on logout.
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.cancel.cancel();
        state.generation = Uuid::new_v4();
        state.status = Status::default();
    }

    /// Return current connector state without exposing credentials.
    pub async fn status(&self) -> Status {
        self.state.lock().await.status.clone()
    }

    async fn run(
        &self,
        center: Url,
        grant: ControlGrant,
        generation: Uuid,
        resume: Option<Uuid>,
    ) -> Result<(), Error> {
        let mut control = self
            .local
            .dialer
            .connect(
                &endpoint(&center, "v1/relay/control"),
                &grant.control_ticket,
                16 * 1024,
            )
            .await?;
        send(
            &mut control,
            Message::Text(
                protocol::encode(&ControlHello::new(
                    grant.node_session_id,
                    &self.local.server_id,
                    &self.local.instance_id,
                    resume,
                ))?
                .into(),
            ),
        )
        .await?;
        let ControlWelcome::Welcome {
            epoch,
            supported_tunnel_protocols,
        } = timeout(Duration::from_secs(5), receive(&mut control))
            .await
            .map_err(|_| Error::Transport)??;
        {
            let mut state = self.state.lock().await;
            if state.generation != generation {
                return Ok(());
            }
            state.status = Status {
                connecting: false,
                online: true,
                epoch: Some(epoch),
                error: None,
            };
        }
        let multiplex = supported_tunnel_protocols
            .iter()
            .any(|name| name == tunnel::PROTOCOL);
        self.serve_commands(&mut control, &center, epoch, multiplex)
            .await
    }

    async fn serve_commands(
        &self,
        control: &mut Socket,
        center: &Url,
        epoch: Uuid,
        multiplex: bool,
    ) -> Result<(), Error> {
        let mut tasks = JoinSet::new();
        let mut active = HashMap::new();
        let (request_tx, mut request_rx) = mpsc::channel(1);
        let (grant_tx, grant_rx) = mpsc::channel(1);
        if multiplex {
            let local = self.local.clone();
            let center = center.clone();
            tasks.spawn(async move {
                tunnel::maintain(local, center, epoch, request_tx, grant_rx).await;
                None
            });
        }
        let idle = tokio::time::sleep(Duration::from_secs(65));
        tokio::pin!(idle);
        loop {
            let message = tokio::select! {
                Some(request) = request_rx.recv(), if multiplex => {
                    send(control, Message::Text(protocol::encode(&request)?.into())).await?;
                    continue;
                },
                result = tasks.join_next(), if !tasks.is_empty() => {
                    match result {
                        Some(Ok(Some(id))) => { active.remove(&id); },
                        Some(Err(error)) => tracing::warn!(panicked = error.is_panic(), "Relay data task did not complete"),
                        Some(Ok(None)) | None => {},
                    }
                    continue;
                },
                () = &mut idle => return Err(Error::Transport),
                message = control.next() => message,
            };
            idle.as_mut()
                .reset(tokio::time::Instant::now() + Duration::from_secs(65));
            let Some(message) = control_command(control, message).await? else {
                continue;
            };
            match message {
                ControlCommand::OpenData(grant) => {
                    let id = grant.relay_session_id;
                    if grant.epoch != epoch || active.contains_key(&id) || active.len() >= 16 {
                        return Err(Error::Protocol);
                    }
                    let url = endpoint(center, &format!("v1/relay/sessions/{id}/daemon"));
                    let local = self.local.clone();
                    let cancel = CancellationToken::new();
                    active.insert(id, cancel.clone());
                    let span = tracing::info_span!("relay_session", relay_session_id = %id, %epoch);
                    tasks.spawn(async move {
                        let started = std::time::Instant::now();
                        tokio::select! {
                            () = cancel.cancelled() => {
                                tracing::info!(reason = "center_cancelled", "Relay session cancelled");
                            },
                            result = async {
                                // Cancellation of a just-ended stream may still be releasing its permit.
                                let _permit = timeout(Duration::from_secs(5), local.visits.clone().acquire_owned())
                                    .await.map_err(|_| Error::Transport)?.map_err(|_| Error::Transport)?;
                                match grant.mode {
                                    DataMode::Download { download_token } =>
                                        download::run(url, grant.daemon_ticket, download_token, local, id).await,
                                    DataMode::RustSingle => bridge::data(url, grant.daemon_ticket, local, id).await,
                                }
                            } => {
                                if let Err(error) = result {
                                    tracing::warn!(%error, "Relay session failed");
                                }
                            },
                        }
                        tracing::info!(elapsed_ms = started.elapsed().as_millis(), "Relay session finished");
                        Some(id)
                    }.instrument(span));
                }
                ControlCommand::CancelSession {
                    relay_session_id: id,
                } => {
                    if let Some(cancel) = active.remove(&id) {
                        cancel.cancel();
                    }
                }
                ControlCommand::TunnelGrant(grant) if multiplex => {
                    let _ = grant_tx.try_send((grant.request_id, Some(grant)));
                }
                ControlCommand::TunnelError { request_id } if multiplex => {
                    let _ = grant_tx.try_send((request_id, None));
                }
                ControlCommand::TunnelGrant(_) | ControlCommand::TunnelError { .. } => {
                    return Err(Error::Protocol);
                }
            }
        }
    }
}

async fn control_command(
    control: &mut Socket,
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<ControlCommand>, Error> {
    match message {
        Some(Ok(Message::Text(body))) => protocol::decode(&body).map(Some),
        Some(Ok(Message::Ping(body))) => {
            send(control, Message::Pong(body)).await?;
            Ok(None)
        }
        Some(Ok(Message::Pong(_))) => Ok(None),
        Some(Err(error)) => {
            diagnostics::socket_error("control_read", &error);
            Err(Error::Transport)
        }
        Some(Ok(Message::Close(frame))) => {
            tracing::info!(
                close_code = frame.as_ref().map(|f| u16::from(f.code)),
                "Relay control closed"
            );
            Err(Error::Transport)
        }
        Some(Ok(Message::Binary(_) | Message::Frame(_))) | None => Err(Error::Transport),
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
mod tests;
