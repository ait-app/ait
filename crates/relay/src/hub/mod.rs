//! Optional `ait-hub-v1` serve endpoint, independent of the legacy control and
//! data connections. One persistent link to the center carries every remote
//! session; each session gets its own WebSocket on the fixed local daemon.
//! The link resumes after network loss without closing those sessions.

mod calls;
mod link;
mod session;
mod wire;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use model::hub::{Center, LinkState, Scope};
use model::share::{Outcome, Visitor};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::time::{Instant, sleep, sleep_until, timeout};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::transport::Socket;
use crate::{Error, Local, center_url, diagnostics, endpoint};
use link::{Joined, Link};
use session::Output;
use wire::{Inbound, Outbound};

/// Sessions per link; the center enforces the same bound.
const MAX_SESSIONS: usize = 16;
/// Buffered business messages per session before it is closed as `overflow`.
const SESSION_QUEUE: usize = 64;
const WELCOME_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// The center pings every 20 seconds; silence beyond this means the link is gone.
const IDLE_TIMEOUT: Duration = Duration::from_secs(65);
const ACK_INTERVAL: Duration = Duration::from_millis(50);
/// Stop resuming slightly before the center discards the link.
const GRACE_MARGIN: Duration = Duration::from_secs(5);
const BACKOFF_MS: [u64; 6] = [250, 500, 1000, 2000, 4000, 8000];
/// After another process took over this node's serve link, wait this long before
/// taking it back, so two processes serving one node never flap.
const REPLACED_BACKOFF: Duration = Duration::from_secs(300);

/// One-use grant for a new hub link, obtained by the Host's account owner.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// HTTPS center origin, or loopback HTTP for development.
    pub center_url: String,
    /// One-use ticket from `POST /v1/node-sessions/{id}/hub-tickets`.
    pub hub_ticket: SecretString,
    /// Node session the ticket was issued for.
    pub node_session_id: Uuid,
}

/// Public link state without credentials.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    /// A welcome was received on the current physical connection.
    pub online: bool,
    /// Dialing or resuming; the link and its sessions still exist.
    pub connecting: bool,
    /// Current link identifier while it exists.
    pub link: Option<Uuid>,
    /// Last failure, without URLs or credentials.
    pub error: Option<String>,
    /// Center error code that ended the last link, e.g. `replaced`.
    pub rejected: Option<String>,
}

#[derive(Debug)]
struct State {
    status: Status,
    generation: Uuid,
    cancel: CancellationToken,
    replaced_at: Option<Instant>,
    calls: Option<calls::Sender>,
}

/// Owns at most one hub link and the local sessions it carries.
#[derive(Clone, Debug)]
pub struct Connector {
    state: Arc<Mutex<State>>,
    local: Arc<Local>,
    shutdown: CancellationToken,
    publication: watch::Sender<LinkState>,
    /// Latest center-initiated share outcome (`share_status` frame).
    shares: watch::Sender<Option<Outcome>>,
}

impl Connector {
    pub(crate) fn new(local: Arc<Local>, shutdown: CancellationToken) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                status: Status::default(),
                generation: Uuid::new_v4(),
                cancel: CancellationToken::new(),
                replaced_at: None,
                calls: None,
            })),
            local,
            shutdown,
            publication: watch::channel(LinkState::default()).0,
            shares: watch::channel(None).0,
        }
    }
}

enum Ended {
    /// The physical connection dropped; the link may resume.
    Lost,
    /// The connector was stopped or replaced.
    Stopped,
}

/// Why a link ended for good.
#[derive(Debug)]
enum Failure {
    /// Transport, credential or protocol failure.
    Relay(Error),
    /// The center ended the link with `error{code}`.
    Rejected(String),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Relay(error)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relay(error) => error.fmt(f),
            Self::Rejected(code) => write!(f, "hub rejected the link: {code}"),
        }
    }
}

impl Connector {
    /// Start a new link, replacing any previous one and its sessions.
    ///
    /// # Errors
    /// Rejects non-TLS remote centers, malformed URLs and malformed tickets.
    pub async fn start(&self, grant: Grant) -> Result<(), Error> {
        if self.shutdown.is_cancelled() {
            return Err(Error::Transport);
        }
        let center = center_url(&grant.center_url)?;
        let ticket = grant.hub_ticket.expose_secret();
        if ticket.len() != 64 || !ticket.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidGrant);
        }
        let generation = Uuid::new_v4();
        let cancel = self.shutdown.child_token();
        let scope = Scope {
            center: center.to_string(),
            node_session_id: grant.node_session_id,
        };
        let (sender, mut calls) = calls::Calls::channel();
        {
            let mut state = self.state.lock().await;
            state.cancel.cancel();
            state.cancel = cancel.clone();
            state.generation = generation;
            state.replaced_at = None;
            state.calls = Some(sender);
            self.publication.send_modify(|publication| {
                if publication.scope.as_ref() != Some(&scope) {
                    publication.methods.clear();
                }
                publication.scope = Some(scope);
                publication.online = false;
            });
            state.status = Status {
                connecting: true,
                ..Status::default()
            };
        }
        let connector = self.clone();
        let span =
            tracing::info_span!("hub_link", %generation, node_session_id = %grant.node_session_id);
        tokio::spawn(
            async move {
                let result = connector
                    .run(
                        &endpoint(&center, "v1/hub"),
                        grant.hub_ticket,
                        generation,
                        &cancel,
                        &mut calls,
                    )
                    .await;
                if let Err(error) = &result {
                    tracing::warn!(%error, "Hub link ended; a new grant is required");
                }
                cancel.cancel();
                connector.finish(generation, result.err()).await;
            }
            .instrument(span),
        );
        Ok(())
    }

    /// Close the link and every session; safe to call repeatedly.
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.cancel.cancel();
        state.generation = Uuid::new_v4();
        state.status = Status::default();
        state.replaced_at = None;
        state.calls = None;
        self.publication.send_replace(LinkState::default());
    }

    /// Current link state without credentials.
    pub async fn status(&self) -> Status {
        self.state.lock().await.status.clone()
    }

    /// Whether another process recently took over this node's serve link; the
    /// owner should not start a new link until this turns false.
    pub async fn yielding(&self) -> bool {
        self.state
            .lock()
            .await
            .replaced_at
            .is_some_and(|at| at.elapsed() < REPLACED_BACKOFF)
    }

    async fn finish(&self, generation: Uuid, failure: Option<Failure>) {
        let mut state = self.state.lock().await;
        if state.generation != generation {
            return;
        }
        let rejected = match &failure {
            Some(Failure::Rejected(code)) => Some(code.clone()),
            Some(Failure::Relay(_)) | None => None,
        };
        if rejected.as_deref() == Some("replaced") {
            state.replaced_at = Some(Instant::now());
        }
        state.status = Status {
            error: failure.map(|failure| failure.to_string()),
            rejected,
            ..Status::default()
        };
        state.calls = None;
        self.publication.send_modify(|publication| {
            publication.online = false;
            if state.status.rejected.is_some() {
                *publication = LinkState::default();
            }
        });
    }

    async fn update(&self, generation: Uuid, change: impl FnOnce(&mut Status)) {
        let mut state = self.state.lock().await;
        if state.generation == generation {
            change(&mut state.status);
        }
    }

    /// Keep one link alive: resume after loss until the center's grace period ends.
    async fn run(
        &self,
        url: &str,
        ticket: SecretString,
        generation: Uuid,
        cancel: &CancellationToken,
        calls: &mut calls::Calls,
    ) -> Result<(), Failure> {
        let mut link = Link::default();
        let (output_tx, mut output_rx) = mpsc::channel(MAX_SESSIONS);
        let mut sessions = Sessions {
            local: self.local.clone(),
            output: output_tx,
            routes: HashMap::new(),
            shares: self.shares.clone(),
        };
        let mut credential = ticket;
        let mut grace = Duration::MAX;
        let mut lost: Option<Instant> = None;
        let mut failures = 0_usize;
        let mut last_failure = Failure::Relay(Error::Transport);
        loop {
            let joined = tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                joined = join(&self.local, url, &credential, &mut link) => joined,
            };
            match joined {
                Ok((socket, fresh, link_grace, methods)) => {
                    if fresh {
                        sessions.routes.clear();
                        calls.fresh();
                    }
                    grace = link_grace;
                    lost = None;
                    failures = 0;
                    let id = link.id();
                    self.update(generation, |status| {
                        *status = Status {
                            online: true,
                            link: id,
                            ..Status::default()
                        };
                    })
                    .await;
                    let state = self.state.lock().await;
                    if state.generation != generation {
                        return Ok(());
                    }
                    self.publication.send_modify(|publication| {
                        publication.online = true;
                        publication.link = id;
                        publication.methods = methods;
                    });
                    drop(state);
                    let mut pump = Pump {
                        link: &mut link,
                        sessions: &mut sessions,
                        outputs: &mut output_rx,
                        calls,
                    };
                    match pump.run(socket, cancel).await? {
                        Ended::Stopped => return Ok(()),
                        Ended::Lost => {}
                    }
                    self.update(generation, |status| {
                        status.online = false;
                        status.connecting = true;
                    })
                    .await;
                    let state = self.state.lock().await;
                    if state.generation == generation {
                        self.publication
                            .send_modify(|publication| publication.online = false);
                    }
                }
                Err(failure @ (Failure::Rejected(_) | Failure::Relay(Error::InvalidGrant))) => {
                    return Err(failure);
                }
                Err(error) => {
                    tracing::info!(%error, failures, "Hub link connect failed");
                    last_failure = error;
                }
            }
            let since = *lost.get_or_insert_with(Instant::now);
            if since.elapsed() + GRACE_MARGIN >= grace {
                return Err(Error::Transport.into());
            }
            // The one-use ticket is spent after the first attempt.
            // Without a welcome there is nothing to resume; report why.
            let Some(token) = link.token().cloned() else {
                return Err(last_failure);
            };
            credential = token;
            let delay = BACKOFF_MS[failures.min(BACKOFF_MS.len() - 1)];
            failures += 1;
            tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                () = sleep(Duration::from_millis(delay)) => {},
            }
        }
    }
}

/// Dial, say hello and apply the welcome. Replays unacknowledged frames on resume.
///
/// Returns whether the link is new, and its grace period. Returns `InvalidGrant`
/// when the center refuses the credential, or `Rejected` when it answers with an
/// error frame, so the caller stops and waits for a fresh grant.
async fn join(
    local: &Local,
    url: &str,
    credential: &SecretString,
    link: &mut Link,
) -> Result<(Socket, bool, Duration, Vec<String>), Failure> {
    let mut socket = local
        .dialer
        .connect_authorized(url, credential.expose_secret(), wire::MAX_FRAME)
        .await?;
    write(&mut socket, Message::Binary(link.hello()?)).await?;
    let frame = timeout(WELCOME_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(frame))) => return Ok(frame),
                Some(Ok(Message::Ping(body))) => write(&mut socket, Message::Pong(body)).await?,
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err(Error::Transport),
            }
        }
    })
    .await
    .map_err(|_| Error::Transport)??;
    match wire::decode(&frame)?.0 {
        Inbound::Welcome {
            link: id,
            resumed,
            recv,
            resume_token,
            grace_seconds,
            center_methods,
        } => {
            let fresh = match link.join(id, resumed, recv, resume_token)? {
                Joined::Fresh => true,
                Joined::Resumed(frames) => {
                    for frame in frames {
                        write(&mut socket, Message::Binary(frame)).await?;
                    }
                    false
                }
            };
            Ok((
                socket,
                fresh,
                Duration::from_secs(grace_seconds),
                center_methods,
            ))
        }
        Inbound::Error { code } => {
            tracing::warn!(code = code.as_str(), "Hub rejected the link");
            Err(Failure::Rejected(code))
        }
        Inbound::Ack { .. }
        | Inbound::Open { .. }
        | Inbound::Msg { .. }
        | Inbound::Reply { .. }
        | Inbound::Close { .. }
        | Inbound::ShareStatus { .. } => Err(Error::Protocol.into()),
    }
}

async fn write(socket: &mut Socket, message: Message) -> Result<(), Error> {
    timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|error| {
            diagnostics::socket_error("hub_write", &error);
            Error::Transport
        })
}

/// Local sessions carried by the link; they outlive physical connections.
struct Sessions {
    local: Arc<Local>,
    output: mpsc::Sender<(Uuid, Output)>,
    routes: HashMap<Uuid, mpsc::Sender<Message>>,
    shares: watch::Sender<Option<Outcome>>,
}

impl Sessions {
    /// Start bridging a session, or return the close code that refuses it.
    /// A `visitor` session is bridged as a guest of one shared workspace.
    fn open(
        &mut self,
        id: Uuid,
        hello: Bytes,
        visitor: Option<&Visitor>,
    ) -> Result<(), &'static str> {
        if self.routes.contains_key(&id) {
            return Ok(());
        }
        if self.routes.len() >= MAX_SESSIONS {
            return Err("capacity");
        }
        let (inbound_tx, inbound_rx) = mpsc::channel(SESSION_QUEUE);
        self.routes.insert(id, inbound_tx);
        let span = tracing::info_span!("hub_session", session = %id);
        tokio::spawn(
            session::run(
                self.local.clone(),
                id,
                hello,
                visitor.map(Visitor::encode),
                inbound_rx,
                self.output.clone(),
            )
            .instrument(span),
        );
        Ok(())
    }

    /// Hand one business message to its session without blocking the link.
    fn deliver(
        &mut self,
        id: Uuid,
        kind: wire::PayloadKind,
        payload: Bytes,
    ) -> Result<(), &'static str> {
        let Some(route) = self.routes.get(&id) else {
            return Ok(());
        };
        let delivered = session::message(kind, payload).and_then(|message| {
            route.try_send(message).map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => "overflow",
                mpsc::error::TrySendError::Closed(_) => "closed",
            })
        });
        if delivered.is_err() {
            self.routes.remove(&id);
        }
        delivered
    }
}

/// Moves frames for one physical connection.
struct Pump<'a> {
    link: &'a mut Link,
    sessions: &'a mut Sessions,
    outputs: &'a mut mpsc::Receiver<(Uuid, Output)>,
    calls: &'a mut calls::Calls,
}

impl Pump<'_> {
    async fn run(&mut self, socket: Socket, cancel: &CancellationToken) -> Result<Ended, Failure> {
        let (mut sink, mut stream) = socket.split();
        let mut acks = tokio::time::interval(ACK_INTERVAL);
        acks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut idle = Instant::now() + IDLE_TIMEOUT;
        loop {
            let outgoing = tokio::select! {
                () = cancel.cancelled() => {
                    let _ = timeout(Duration::from_secs(1), sink.close()).await;
                    return Ok(Ended::Stopped);
                }
                message = stream.next() => {
                    idle = Instant::now() + IDLE_TIMEOUT;
                    match message {
                        Some(Ok(Message::Binary(frame))) => self.inbound(&frame)?,
                        Some(Ok(Message::Ping(body))) => Some(Message::Pong(body)),
                        Some(Ok(Message::Pong(_))) => None,
                        Some(Ok(Message::Text(_) | Message::Frame(_))) => {
                            return Err(Error::Protocol.into());
                        }
                        Some(Err(error)) => {
                            diagnostics::socket_error("hub_read", &error);
                            return Ok(Ended::Lost);
                        }
                        Some(Ok(Message::Close(_))) | None => return Ok(Ended::Lost),
                    }
                }
                Some((id, output)) = self.outputs.recv(), if self.link.has_room() => self.output(id, output)?,
                Some(request) = self.calls.queue.recv(), if self.link.has_room() => {
                    self.calls.output(request, self.link)?.map(Message::Binary)
                }
                _ = acks.tick() => {
                    self.calls.prune();
                    self.link.take_ack()?.map(Message::Binary)
                },
                () = sleep_until(idle) => return Ok(Ended::Lost),
            };
            if let Some(message) = outgoing
                && !matches!(timeout(WRITE_TIMEOUT, sink.send(message)).await, Ok(Ok(())))
            {
                return Ok(Ended::Lost);
            }
        }
    }

    /// Apply one center frame; returns a frame to send in reply, if any.
    fn inbound(&mut self, frame: &Bytes) -> Result<Option<Message>, Failure> {
        let (header, payload) = wire::decode(frame)?;
        if let Some(seq) = header.seq()
            && !self.link.received(seq)
        {
            return Ok(None);
        }
        let refused = match header {
            Inbound::Ack { a } => {
                self.link.acked(a)?;
                return Ok(None);
            }
            Inbound::Reply { id, .. } => {
                self.calls.reply(id, &payload)?;
                return Ok(None);
            }
            Inbound::Open { s, visitor, .. } => self
                .sessions
                .open(s, payload, visitor.as_ref())
                .err()
                .map(|code| (s, code)),
            Inbound::ShareStatus { .. } => {
                // A malformed outcome is ignored; the next publication repairs state.
                if let Ok(outcome) = serde_json::from_slice::<Outcome>(&payload) {
                    self.sessions.shares.send_replace(Some(outcome));
                }
                return Ok(None);
            }
            Inbound::Msg { s, pt, .. } => self
                .sessions
                .deliver(s, pt, payload)
                .err()
                .map(|code| (s, code)),
            Inbound::Close { s, .. } => {
                self.sessions.routes.remove(&s);
                None
            }
            Inbound::Error { code } => {
                tracing::warn!(code = code.as_str(), "Hub closed the link");
                return Err(Failure::Rejected(code));
            }
            Inbound::Welcome { .. } => return Err(Error::Protocol.into()),
        };
        let Some((s, code)) = refused else {
            return Ok(None);
        };
        let frame = self
            .link
            .push(|seq| Outbound::Close { seq, s, code }, b"")?;
        Ok(Some(Message::Binary(frame)))
    }

    /// Frame one session output; output of an already closed session is dropped.
    fn output(&mut self, id: Uuid, output: Output) -> Result<Option<Message>, Error> {
        let frame = match output {
            Output::Closed(code) => {
                if self.sessions.routes.remove(&id).is_none() {
                    return Ok(None);
                }
                self.link
                    .push(|seq| Outbound::Close { seq, s: id, code }, b"")?
            }
            _ if !self.sessions.routes.contains_key(&id) => return Ok(None),
            Output::Accept(info) => self
                .link
                .push(|seq| Outbound::Accept { seq, s: id }, &info)?,
            Output::Message(pt, body) => self
                .link
                .push(|seq| Outbound::Msg { seq, s: id, pt }, &body)?,
        };
        Ok(Some(Message::Binary(frame)))
    }
}

#[async_trait::async_trait]
impl Center for Connector {
    fn state(&self) -> watch::Receiver<LinkState> {
        self.publication.subscribe()
    }

    fn share_status(&self) -> watch::Receiver<Option<Outcome>> {
        self.shares.subscribe()
    }

    async fn call(
        &self,
        scope: Scope,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, model::hub::CallError> {
        self.call_center(scope, method, params).await
    }
}

#[cfg(test)]
mod tests;
