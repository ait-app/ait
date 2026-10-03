//! Outbound control and reverse data transport. Local destinations are fixed at construction.
mod bridge;
mod download;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, protocol::WebSocketConfig,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const MAX_MESSAGE: usize = 1024 * 1024;

/// A short-lived credential handed over by the trusted desktop account manager.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlGrant {
    /// HTTPS center base URL; HTTP is accepted only for loopback development.
    pub center_url: String,
    /// One-use control ticket, never a user JWT.
    pub control_ticket: String,
    /// Node activation associated with this ticket.
    pub node_session_id: Uuid,
}

/// Non-secret connector state exposed to the account manager.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// Whether a control attempt is still in progress.
    pub connecting: bool,
    /// Whether the center accepted this Host's control hello.
    pub online: bool,
    /// Last accepted routing generation, used only to replace the same instance.
    pub epoch: Option<Uuid>,
    /// Non-secret machine-readable failure category.
    pub error: Option<String>,
}

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
        tokio::spawn(async move {
            let result = tokio::select! {
                () = cancel.cancelled() => Ok(()),
                result = connector.run(center, grant, generation, resume) => result,
            };
            cancel.cancel();
            let mut state = connector.state.lock().await;
            if state.generation == generation {
                state.status.connecting = false;
                state.status.online = false;
                state.status.error = result.err().map(|error| error.to_string());
            }
        });
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
        let mut control = connect(
            &endpoint(&center, "v1/relay/control"),
            &grant.control_ticket,
            16 * 1024,
        )
        .await?;
        send(
            &mut control,
            Message::Text(
                json!({"type":"control.hello","version":1,
            "node_session_id":grant.node_session_id,"server_id":self.local.server_id,
            "instance_id":self.local.instance_id,"resume_epoch":resume})
                .to_string()
                .into(),
            ),
        )
        .await?;
        let welcome = timeout(Duration::from_secs(5), text(&mut control))
            .await
            .map_err(|_| Error::Transport)??;
        if welcome["type"] != "control.welcome" {
            return Err(Error::Protocol);
        }
        let epoch = welcome["epoch"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Protocol)?;
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
        self.serve_commands(&mut control, &center, epoch).await
    }

    async fn serve_commands(
        &self,
        control: &mut Socket,
        center: &Url,
        epoch: Uuid,
    ) -> Result<(), Error> {
        let mut tasks = JoinSet::new();
        let mut active = HashMap::new();
        loop {
            let message = tokio::select! {
                result = tasks.join_next(), if !tasks.is_empty() => {
                    if let Some(Ok(id)) = result { active.remove(&id); }
                    continue;
                },
                message = timeout(Duration::from_secs(65), control.next()) => match message {
                    Ok(Some(Ok(Message::Text(value)))) => serde_json::from_str::<serde_json::Value>(&value).map_err(|_| Error::Protocol)?,
                    Ok(Some(Ok(Message::Ping(value)))) => { send(control, Message::Pong(value)).await?; continue; },
                    Ok(Some(Ok(Message::Pong(_)))) => continue,
                    _ => return Err(Error::Transport),
                },
            };
            let id = message["relay_session_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(Error::Protocol)?;
            match message["type"].as_str() {
                Some("open_data") => {
                    if message["epoch"] != epoch.to_string()
                        || !matches!(
                            message["mode"].as_str(),
                            Some("ait-rust-single-v1" | "ait-download-v1")
                        )
                        || active.contains_key(&id)
                        || active.len() >= 16
                    {
                        return Err(Error::Protocol);
                    }
                    let ticket = message["daemon_ticket"]
                        .as_str()
                        .ok_or(Error::Protocol)?
                        .to_owned();
                    let url = endpoint(center, &format!("v1/relay/sessions/{id}/daemon"));
                    let local = self.local.clone();
                    let download_token = if message["mode"] == "ait-download-v1" {
                        Some(
                            message["download_token"]
                                .as_str()
                                .ok_or(Error::Protocol)?
                                .to_owned(),
                        )
                    } else {
                        None
                    };
                    let cancel = CancellationToken::new();
                    active.insert(id, cancel.clone());
                    tasks.spawn(async move {
                        tokio::select! {
                            () = cancel.cancelled() => {},
                            _ = async {
                                if let Some(token) = download_token { download::run(url,ticket,token,local).await }
                                else { bridge::data(url,ticket,local).await }
                            } => {},
                        }
                        id
                    });
                }
                Some("cancel_session") => {
                    if let Some(cancel) = active.remove(&id) {
                        cancel.cancel();
                    }
                }
                _ => return Err(Error::Protocol),
            }
        }
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

async fn connect(url: &str, ticket: &str, max: usize) -> Result<Socket, Error> {
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
        connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| Error::Transport)?
    .map(|(socket, _)| socket)
    .map_err(|_| Error::Transport)
}

async fn send(socket: &mut Socket, message: Message) -> Result<(), Error> {
    timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)
}

async fn text(socket: &mut Socket) -> Result<serde_json::Value, Error> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|_| Error::Protocol);
            }
            Some(Ok(Message::Ping(bytes))) => send(socket, Message::Pong(bytes)).await?,
            Some(Ok(Message::Pong(_))) => {}
            _ => return Err(Error::Transport),
        }
    }
}

#[cfg(test)]
mod tests;
