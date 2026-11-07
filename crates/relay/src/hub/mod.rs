//! `ait-hub-v1` serve endpoint. One persistent link to the center carries every
//! remote session; each session is served in-process by the attached local API,
//! or streams a file download. The link resumes after network loss without
//! closing those sessions. With feature `bulk`, downloads may instead arrive on
//! a second, download-only link the center asks for.
//!
//! The link always declares `lease`, `credit` and `download` (and `bulk` when
//! the owner can obtain bulk tickets). A center that does not declare or enable
//! all of them predates this daemon; the link fails instead of downgrading.
//! Sessions still adapt to their opener: an `open` without `w` is served without
//! flow control, one without `k` is an RPC session.

pub mod bridge;
pub mod bulk;
mod calls;
mod credit;
mod link;
mod session;
mod wire;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
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
use crate::{Error, Local, center_url, diagnostics, endpoint, validate_ticket};
use bridge::Frame;
use link::{Joined, Link};
use session::{Output, Session};
use wire::{Inbound, Outbound, SessionKind};

/// Sessions per link; the center enforces the same bound.
const MAX_SESSIONS: usize = 16;
/// Buffered business messages per session before it is closed as `overflow`.
const SESSION_QUEUE: usize = 64;
/// Message bound of a flow-controlled session. Its window already bounds the
/// bytes; this only guards against floods of empty messages.
const CREDIT_QUEUE: usize = 4096;
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
    /// Features the center declared over HTTP with the ticket; it must include
    /// every feature the link declares.
    pub hub_features: Vec<String>,
    /// Source of bulk lane tickets; without one the link never declares `bulk`.
    #[serde(skip)]
    pub tickets: Option<Arc<dyn bulk::Tickets>>,
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
    /// Features the center enabled on the current link (`welcome.f`).
    pub features: Vec<String>,
}

impl Status {
    /// Whether a link exists, attached or resuming; it then holds the node
    /// session lease (`lease`, which every link enables), so the owner must not
    /// renew it over HTTP.
    #[must_use]
    pub const fn holds_lease(&self) -> bool {
        self.link.is_some()
    }
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

/// What a link carries.
enum Role {
    /// The main serve link of `generation`: it reports status and publication,
    /// carries calls and every session kind, and opens the bulk link on `lane`
    /// hints when it declared `bulk`.
    Main {
        generation: Uuid,
        lane: Option<bulk::Lane>,
    },
    /// A download-only bulk link. The center closes it with `idle` when unused,
    /// so a download is never routed to a link the Host is closing. It never
    /// touches the connector's status, so an `idle` or `replaced` bulk link simply ends.
    Bulk,
}

/// Why a link ended for good.
#[derive(Debug)]
enum Failure {
    /// Transport, credential or protocol failure.
    Relay(Error),
    /// The center ended the link with `error{code}`.
    Rejected(String),
    /// The center did not enable this feature the link declared.
    Feature(&'static str),
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
            Self::Feature(feature) => write!(f, "hub center lacks feature {feature}"),
        }
    }
}

impl Connector {
    /// Start a new link, replacing any previous one and its sessions.
    ///
    /// # Errors
    /// Rejects non-TLS remote centers, malformed URLs and malformed tickets as
    /// `InvalidGrant`, and a center that did not declare every feature the link
    /// needs (a center predating them) as `Protocol`.
    pub async fn start(&self, grant: Grant) -> Result<(), Error> {
        if self.shutdown.is_cancelled() {
            return Err(Error::Transport);
        }
        let center = center_url(&grant.center_url)?;
        validate_ticket(grant.hub_ticket.expose_secret())?;
        let offer = wire::offer(grant.tickets.is_some());
        if let Some(feature) = wire::missing(&offer, &grant.hub_features) {
            tracing::error!(
                feature,
                declared = ?grant.hub_features,
                "Hub center lacks a required feature; this daemon needs a newer center"
            );
            return Err(Error::Protocol);
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
        let url = endpoint(&center, "v1/hub");
        let lane = grant.tickets.map(|tickets| bulk::Lane {
            connector: self.clone(),
            tickets,
            url: url.clone(),
            cancel: cancel.clone(),
            open: Arc::new(AtomicBool::new(false)),
        });
        tokio::spawn(
            async move {
                let role = Role::Main { generation, lane };
                let result = connector
                    .run(
                        &url,
                        (grant.hub_ticket, Link::new(offer)),
                        &role,
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
            Some(Failure::Relay(_) | Failure::Feature(_)) | None => None,
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

    /// Publish a welcomed main link; returns `false` once the link was superseded.
    async fn welcomed(&self, generation: Uuid, link: Option<Uuid>, welcome: &mut Welcome) -> bool {
        let features = std::mem::take(&mut welcome.features);
        self.update(generation, |status| {
            *status = Status {
                online: true,
                link,
                features,
                ..Status::default()
            };
        })
        .await;
        let state = self.state.lock().await;
        if state.generation != generation {
            return false;
        }
        let methods = std::mem::take(&mut welcome.methods);
        self.publication.send_modify(|publication| {
            publication.online = true;
            publication.link = link;
            publication.methods = methods;
        });
        true
    }

    /// Report a main link that lost its connection and is resuming.
    async fn resuming(&self, generation: Uuid) {
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

    /// Keep one link alive: resume after loss until the center's grace period ends.
    /// The ticket is spent on the first connection; `link` carries the offered features.
    async fn run(
        &self,
        url: &str,
        (ticket, mut link): (SecretString, Link),
        role: &Role,
        cancel: &CancellationToken,
        calls: &mut calls::Calls,
    ) -> Result<(), Failure> {
        let (output_tx, mut output_rx) = mpsc::channel(MAX_SESSIONS);
        let mut sessions = Sessions {
            local: self.local.clone(),
            output: output_tx,
            routes: HashMap::new(),
            shares: self.shares.clone(),
            downloads_only: matches!(role, Role::Bulk),
        };
        let lane = match role {
            Role::Main { lane, .. } => lane.as_ref(),
            Role::Bulk => None,
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
                Ok(mut welcome) => {
                    if welcome.fresh {
                        sessions.routes.clear();
                        calls.fresh();
                    }
                    grace = welcome.grace;
                    lost = None;
                    failures = 0;
                    if let Role::Main { generation, .. } = role
                        && !self.welcomed(*generation, link.id(), &mut welcome).await
                    {
                        return Ok(());
                    }
                    // Empty on a fresh link, whose sessions are gone.
                    let credits = sessions_credit(&sessions);
                    let mut pump = Pump {
                        link: &mut link,
                        sessions: &mut sessions,
                        outputs: &mut output_rx,
                        calls,
                        lane,
                    };
                    match pump.run(welcome.socket, credits, cancel).await? {
                        Ended::Stopped => return Ok(()),
                        Ended::Lost => {}
                    }
                    if let Role::Main { generation, .. } = role {
                        self.resuming(*generation).await;
                    }
                }
                Err(
                    failure @ (Failure::Rejected(_)
                    | Failure::Feature(_)
                    | Failure::Relay(Error::InvalidGrant)),
                ) => {
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

/// A connection the center welcomed.
struct Welcome {
    socket: Socket,
    /// A new link: sessions of an earlier link are gone.
    fresh: bool,
    grace: Duration,
    methods: Vec<String>,
    /// `welcome.f`: features enabled on the link.
    features: Vec<String>,
}

/// Dial, say hello and apply the welcome. Replays unacknowledged frames on resume.
///
/// Returns `InvalidGrant` when the center refuses the credential, `Rejected`
/// when it answers with an error frame, or `Feature` when its welcome does not
/// enable every offered feature, so the caller stops and waits for a fresh grant.
async fn join(
    local: &Local,
    url: &str,
    credential: &SecretString,
    link: &mut Link,
) -> Result<Welcome, Failure> {
    let mut socket = local
        .dialer
        .connect(url, credential.expose_secret(), wire::MAX_FRAME)
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
            f,
        } => {
            // A center predating features ignores `hello.f`; never run a link
            // whose lease or flow control the center does not honor.
            if let Some(feature) = wire::missing(link.offer(), &f) {
                tracing::error!(
                    feature,
                    enabled = ?f,
                    "Hub welcome lacks a required feature; this daemon needs a newer center"
                );
                return Err(Failure::Feature(feature));
            }
            let fresh = match link.join(id, resumed, recv, resume_token)? {
                Joined::Fresh => true,
                Joined::Resumed(frames) => {
                    for frame in frames {
                        write(&mut socket, Message::Binary(frame)).await?;
                    }
                    false
                }
            };
            Ok(Welcome {
                socket,
                fresh,
                grace: Duration::from_secs(grace_seconds),
                methods: center_methods,
                features: f,
            })
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
        | Inbound::ShareStatus { .. }
        | Inbound::Credit { .. }
        | Inbound::Lane { .. } => Err(Error::Protocol.into()),
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
    routes: HashMap<Uuid, Route>,
    shares: watch::Sender<Option<Outcome>>,
    /// A bulk link refuses every session but downloads.
    downloads_only: bool,
}

/// The link's handle on one session task.
struct Route {
    inbound: mpsc::Sender<Frame>,
    /// Flow control of a session opened with a window.
    credit: Option<RouteCredit>,
}

struct RouteCredit {
    /// Latest cumulative credit from the peer; dropping it ends a waiting sender.
    peer: watch::Sender<u64>,
    /// Latest cumulative credit this Host announced, repeated after a resume.
    granted: u64,
}

/// An `open` frame's session parameters.
struct Opening<'a> {
    visitor: Option<&'a Visitor>,
    kind: SessionKind,
    /// The opener's receive window; present only on flow-controlled sessions.
    window: Option<u32>,
}

impl Sessions {
    /// Start a session, or return the close code that refuses it.
    /// A `visitor` session is served as a guest of one shared resource.
    fn open(
        &mut self,
        id: Uuid,
        payload: Bytes,
        opening: &Opening<'_>,
    ) -> Result<(), &'static str> {
        if self.routes.contains_key(&id) {
            return Ok(());
        }
        if self.routes.len() >= MAX_SESSIONS {
            return Err("capacity");
        }
        if opening.kind == SessionKind::Unsupported
            || (self.downloads_only && opening.kind != SessionKind::Download)
        {
            return Err("unsupported");
        }
        let (credit, peer) = match opening.window {
            Some(window) => {
                let (peer, credit) = watch::channel(0);
                (
                    Some(credit::Outgoing::new(credit, window)),
                    Some(RouteCredit { peer, granted: 0 }),
                )
            }
            None => (None, None),
        };
        let queue = if peer.is_some() {
            CREDIT_QUEUE
        } else {
            SESSION_QUEUE
        };
        let (inbound_tx, inbound_rx) = mpsc::channel(queue);
        self.routes.insert(
            id,
            Route {
                inbound: inbound_tx,
                credit: peer,
            },
        );
        let span = tracing::info_span!("hub_session", session = %id);
        let session = Session {
            id,
            local: self.local.clone(),
            visitor: opening.visitor.cloned(),
            inbound: inbound_rx,
            output: self.output.clone(),
            credit,
        };
        tokio::spawn(session::run(session, opening.kind, payload).instrument(span));
        Ok(())
    }

    /// Hand one business message to its session without blocking the link.
    /// A flow-controlled session is bounded by its window instead of the
    /// message count, so its peer is never closed for sending within credit.
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
            route
                .inbound
                .try_send(message)
                .map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => "overflow",
                    mpsc::error::TrySendError::Closed(_) => "closed",
                })
        });
        if delivered.is_err() {
            self.routes.remove(&id);
        }
        delivered
    }

    /// Record the peer's cumulative credit; it never decreases.
    fn credit(&self, id: Uuid, credit: u64) {
        if let Some(peer) = self.routes.get(&id).and_then(|route| route.credit.as_ref()) {
            peer.peer.send_if_modified(|current| {
                let raised = credit > *current;
                if raised {
                    *current = credit;
                }
                raised
            });
        }
    }
}

/// Latest credit of every flow-controlled session, repeated once after a resume.
fn sessions_credit(sessions: &Sessions) -> Vec<(Uuid, u64)> {
    sessions
        .routes
        .iter()
        .filter_map(|(id, route)| route.credit.as_ref().map(|credit| (*id, credit.granted)))
        .filter(|(_, granted)| *granted > 0)
        .collect()
}

/// Moves frames for one physical connection.
struct Pump<'a> {
    link: &'a mut Link,
    sessions: &'a mut Sessions,
    outputs: &'a mut mpsc::Receiver<(Uuid, Output)>,
    calls: &'a mut calls::Calls,
    /// Opens the bulk link on `lane` hints; only on a main link that declared `bulk`.
    lane: Option<&'a bulk::Lane>,
}

impl Pump<'_> {
    /// Move frames until the connection ends. `credits` holds the latest credit
    /// of each flow-controlled session, announced again first after a resume.
    async fn run(
        &mut self,
        socket: Socket,
        credits: Vec<(Uuid, u64)>,
        cancel: &CancellationToken,
    ) -> Result<Ended, Failure> {
        let (mut sink, mut stream) = socket.split();
        for (s, c) in credits {
            let frame = wire::encode(&Outbound::Credit { s, c }, b"")?;
            if !matches!(
                timeout(WRITE_TIMEOUT, sink.send(Message::Binary(frame))).await,
                Ok(Ok(()))
            ) {
                return Ok(Ended::Lost);
            }
        }
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
            Inbound::Open {
                s, visitor, k, w, ..
            } => {
                let opening = Opening {
                    visitor: visitor.as_ref(),
                    kind: k,
                    window: w,
                };
                self.sessions
                    .open(s, payload, &opening)
                    .err()
                    .map(|code| (s, code))
            }
            Inbound::Credit { s, c } => {
                self.sessions.credit(s, c);
                return Ok(None);
            }
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
            Inbound::Lane { lane } => {
                // Unknown lanes are ignored; a link without bulk tickets never declared `bulk`.
                if lane == wire::BULK
                    && let Some(lane) = self.lane
                {
                    lane.hint();
                }
                return Ok(None);
            }
            Inbound::Error { code } => {
                // The center closes unused bulk links as `idle`; that is routine.
                if code == "idle" {
                    tracing::info!("Hub closed the idle link");
                } else {
                    tracing::warn!(code = code.as_str(), "Hub closed the link");
                }
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
        if let Output::Closed(code) = output {
            if self.sessions.routes.remove(&id).is_none() {
                return Ok(None);
            }
            let frame = self
                .link
                .push(|seq| Outbound::Close { seq, s: id, code }, b"")?;
            return Ok(Some(Message::Binary(frame)));
        }
        let Some(route) = self.sessions.routes.get_mut(&id) else {
            return Ok(None);
        };
        let frame = match output {
            // Answer a window with this Host's own; otherwise the session is not flow-controlled.
            Output::Accept(info) => {
                let w = route.credit.as_ref().map(|_| credit::WINDOW);
                self.link
                    .push(|seq| Outbound::Accept { seq, s: id, w }, &info)?
            }
            Output::Credit(c) => {
                let Some(credit) = route.credit.as_mut() else {
                    return Ok(None);
                };
                credit.granted = c;
                wire::encode(&Outbound::Credit { s: id, c }, b"")?
            }
            Output::Message(pt, body) => self
                .link
                .push(|seq| Outbound::Msg { seq, s: id, pt }, &body)?,
            Output::Closed(_) => return Ok(None),
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
