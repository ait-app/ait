//! The outbound connection: handshake, hello, heartbeat, close codes and backoff.
//!
//! State machine: `Connecting → HelloSent → Ready → (Closed → Backoff → Connecting)`, or
//! `Halted` after `4401`, a handshake `401` or `4409`. Nothing but the hello is sent before
//! `runtime.welcome`; after it the runtime only answers what the Hub asks.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures_util::SinkExt;
use futures_util::stream::{SplitSink, StreamExt};
use secrecy::ExposeSecret;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode, header};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::hello::{self, Offer};
use crate::outbox::{LastFrame, Outbox, Outgoing, Sink, Writer};
use crate::ports::{BoxFuture, Host};
use crate::runs::Input;
use crate::store::Store;
use crate::wire::{self, Inbound, Incoming, PONG};

/// Missing `pong` for this long means the connection is dead (the writer pings every
/// [`crate::outbox::HEARTBEAT`]).
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_mins(1);
/// Longest backoff between attempts.
pub const BACKOFF_CAP: Duration = Duration::from_mins(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const WELCOME_TIMEOUT: Duration = Duration::from_secs(20);
const OFFER_CHECK: Duration = Duration::from_mins(1);
const OFFER_DEBOUNCE: Duration = Duration::from_secs(30);
/// Provider availability goes through AIT's provider catalog, whose 60-second cache makes each
/// query probe every installed provider again, so offer checks ask for it this rarely; every
/// reconnect rediscovers anyway.
const PROVIDER_CHECK: Duration = Duration::from_mins(15);
/// How long a closing connection's writer may take to finish (a dead TCP peer can block a
/// send until the kernel gives up, for many minutes).
const WRITER_DRAIN: Duration = Duration::from_secs(5);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Why a connection attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// Try again after the backoff (at least `at_least` when the Hub asked for it).
    Retry {
        /// Minimum wait requested by `Retry-After`.
        at_least: Option<Duration>,
        /// The connection had reached `runtime.welcome`.
        welcomed: bool,
    },
    /// Never reconnect.
    Halt(Halt),
    /// The service is stopping.
    Stopped,
}

/// Why the connection stopped for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    /// `4401` or a handshake `401`: revoked or unknown credentials; Bonsai sessions must stop.
    Revoked,
    /// `4409`: another process connected with the same runtime ID.
    Replaced,
    /// A local configuration problem (endpoint, token, hello too large).
    Misconfigured,
}

/// What to do after a close code (protocol §2).
#[must_use]
pub fn after_close(code: u16) -> Exit {
    match code {
        4401 => Exit::Halt(Halt::Revoked),
        4409 => Exit::Halt(Halt::Replaced),
        _ => Exit::Retry {
            at_least: None,
            welcomed: true,
        },
    }
}

/// What to do after a rejected handshake.
#[must_use]
pub fn after_handshake(status: StatusCode, retry_after: Option<&HeaderValue>) -> Exit {
    match status.as_u16() {
        401 => Exit::Halt(Halt::Revoked),
        503 => Exit::Retry {
            at_least: retry_after
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(Duration::from_secs),
            welcomed: false,
        },
        _ => Exit::Retry {
            at_least: None,
            welcomed: false,
        },
    }
}

/// Exponential backoff from one second, doubling to [`BACKOFF_CAP`], with jitter.
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    /// The next wait; `at_least` raises it (still capped).
    pub fn next(&mut self, at_least: Option<Duration>) -> Duration {
        let full = Duration::from_secs(1u64 << self.attempt.min(6)).min(BACKOFF_CAP);
        self.attempt = self.attempt.saturating_add(1);
        let jitter = 0.5 + 0.5 * (f64::from(uuid::Uuid::new_v4().as_fields().0 % 1000) / 1000.0);
        full.mul_f64(jitter)
            .max(at_least.unwrap_or_default())
            .min(BACKOFF_CAP)
    }

    /// Clear after `runtime.welcome`.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

struct SocketSink(SplitSink<Socket, Message>);

impl Sink for SocketSink {
    fn send(&mut self, text: String) -> BoxFuture<'_, Result<(), ()>> {
        Box::pin(async move {
            self.0
                .send(Message::Text(text.into()))
                .await
                .map_err(|_| ())
        })
    }

    fn close(&mut self, code: u16) -> BoxFuture<'_, Result<(), ()>> {
        Box::pin(async move { self.0.send(close_frame(code)).await.map_err(|_| ()) })
    }
}

/// The connection loop and what it shares with the rest of the adapter.
pub(crate) struct Link {
    pub(crate) config: Config,
    pub(crate) host: Host,
    pub(crate) store: Store,
    pub(crate) outbox: Outbox,
    pub(crate) offer: Arc<RwLock<Offer>>,
    pub(crate) coordinator: mpsc::UnboundedSender<Input>,
    pub(crate) name: String,
    pub(crate) policy: crate::runs::WritePolicy,
    pub(crate) cancel: CancellationToken,
    pub(crate) last: Arc<Mutex<LastFrame>>,
}

impl Link {
    /// Connect, serve and reconnect until halted or stopped.
    pub(crate) async fn run(self) {
        let mut backoff = Backoff::default();
        let mut stale = true;
        loop {
            if stale {
                tokio::select! {
                    biased;
                    () = self.refresh_offer() => {}
                    () = self.cancel.cancelled() => return,
                }
            }
            let exit = self.connect_once(&mut backoff).await;
            match exit {
                Exit::Stopped => return,
                Exit::Halt(reason) => {
                    tracing::warn!(
                        ?reason,
                        "Bonsai runtime connection halted; not reconnecting"
                    );
                    if reason == Halt::Revoked {
                        let _ = self.coordinator.send(Input::Revoked);
                    }
                    return;
                }
                Exit::Retry { at_least, welcomed } => {
                    stale = welcomed;
                    let wait = backoff.next(at_least);
                    tokio::select! {
                        () = self.cancel.cancelled() => return,
                        () = tokio::time::sleep(wait) => {}
                    }
                }
            }
        }
    }

    async fn refresh_offer(&self) {
        match hello::discover(
            self.host.executor.as_ref(),
            self.host.projects.as_ref(),
            self.name.clone(),
            &|provider: &str| self.policy.bonsai_write(provider),
        )
        .await
        {
            Ok(offer) => {
                *self
                    .offer
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = offer;
            }
            Err(error) => tracing::warn!(?error, "Bonsai runtime discovery failed"),
        }
    }

    async fn connect_once(&self, backoff: &mut Backoff) -> Exit {
        let Ok(mut request) = self.config.endpoint().as_str().into_client_request() else {
            return Exit::Halt(Halt::Misconfigured);
        };
        let Ok(mut authorization) =
            HeaderValue::from_str(&format!("Bearer {}", self.config.token().expose_secret()))
        else {
            return Exit::Halt(Halt::Misconfigured);
        };
        authorization.set_sensitive(true);
        request
            .headers_mut()
            .insert(header::AUTHORIZATION, authorization);
        let connected = tokio::select! {
            () = self.cancel.cancelled() => return Exit::Stopped,
            connected = tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async(request)) => connected,
        };
        let socket = match connected {
            Ok(Ok((socket, _))) => socket,
            Ok(Err(WsError::Http(response))) => {
                tracing::warn!(
                    status = response.status().as_u16(),
                    "Bonsai rejected the runtime handshake"
                );
                return after_handshake(
                    response.status(),
                    response.headers().get(header::RETRY_AFTER),
                );
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, "Bonsai runtime connection failed");
                return Exit::Retry {
                    at_least: None,
                    welcomed: false,
                };
            }
            Err(_) => {
                return Exit::Retry {
                    at_least: None,
                    welcomed: false,
                };
            }
        };
        // The last frame of an earlier connection must never be quarantined for this one.
        *self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = LastFrame {
            kind: "runtime.hello".to_owned(),
            ..LastFrame::default()
        };
        let (mut sink, mut stream) = socket.split();
        let fitted = self
            .offer
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hello_frame(self.config.runtime_id());
        let Some((hello, dropped)) = fitted else {
            tracing::error!("runtime.hello exceeds the protocol limit even without projects");
            return Exit::Halt(Halt::Misconfigured);
        };
        if dropped > 0 {
            tracing::warn!(
                dropped,
                "runtime.hello left out projects or models to fit its limit"
            );
        }
        if sink
            .send(Message::Text(hello.clone().into()))
            .await
            .is_err()
        {
            return Exit::Retry {
                at_least: None,
                welcomed: false,
            };
        }
        let owner = match self.welcome(&mut stream).await {
            Ok(owner) => owner,
            Err(exit) => return exit,
        };
        backoff.reset();
        tracing::info!("Bonsai runtime connected");
        let Ok(writer) = Writer::new(SocketSink(sink), self.store.clone(), Arc::clone(&self.last))
        else {
            return Exit::Retry {
                at_least: None,
                welcomed: true,
            };
        };
        let (sender, queue) = mpsc::unbounded_channel();
        self.outbox.attach(sender);
        let writer = tokio::spawn(writer.run(queue));
        let _ = self.coordinator.send(Input::Connected(owner));
        let exit = self.serve(&mut stream, &hello).await;
        self.outbox.detach();
        drain_writer(writer).await;
        exit
    }

    async fn welcome(
        &self,
        stream: &mut futures_util::stream::SplitStream<Socket>,
    ) -> Result<wire::Person, Exit> {
        let deadline = Instant::now() + WELCOME_TIMEOUT;
        loop {
            let message = tokio::select! {
                () = self.cancel.cancelled() => return Err(Exit::Stopped),
                () = tokio::time::sleep_until(deadline) => return Err(Exit::Retry { at_least: None, welcomed: false }),
                message = stream.next() => message,
            };
            match message {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(Incoming::Frame(Inbound::Welcome(welcome))) = wire::decode(&text) {
                        return Ok(welcome.owner);
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    return Err(close_exit(frame.as_ref(), &self.last, &self.coordinator));
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    return Err(Exit::Retry {
                        at_least: None,
                        welcomed: false,
                    });
                }
            }
        }
    }

    async fn serve(
        &self,
        stream: &mut futures_util::stream::SplitStream<Socket>,
        sent_hello: &str,
    ) -> Exit {
        let mut offer_check = tokio::time::interval_at(Instant::now() + OFFER_CHECK, OFFER_CHECK);
        let mut providers_checked = Instant::now();
        let mut with_providers = false;
        let mut last_pong = Instant::now();
        let mut changed_at: Option<Instant> = None;
        // The check awaits AIT's worker; it runs aside so inbound frames keep flowing. A change
        // is acted on only when a second check after the debounce still sees it.
        let mut checking: Option<Check> = None;
        loop {
            let reconnect_at = changed_at
                .filter(|_| checking.is_none())
                .map(|at| at + OFFER_DEBOUNCE);
            tokio::select! {
                () = self.cancel.cancelled() => {
                    self.outbox.send(Outgoing::Close(1000));
                    return Exit::Stopped;
                }
                () = tokio::time::sleep_until(last_pong + HEARTBEAT_TIMEOUT) => {
                    tracing::warn!("Bonsai runtime heartbeat timed out");
                    return Exit::Retry { at_least: None, welcomed: true };
                }
                _ = offer_check.tick(), if checking.is_none() && changed_at.is_none() => {
                    with_providers = providers_checked.elapsed() >= PROVIDER_CHECK;
                    if with_providers {
                        providers_checked = Instant::now();
                    }
                    checking = Some(self.check_offer(sent_hello, false, with_providers));
                }
                (changed, confirming) = finished(checking.as_mut()) => {
                    checking = None;
                    if !changed {
                        changed_at = None;
                    } else if confirming {
                        tracing::info!("runtime projects or providers changed; reconnecting with a new hello");
                        self.outbox.send(Outgoing::Close(1000));
                        return Exit::Retry { at_least: None, welcomed: true };
                    } else {
                        changed_at = Some(Instant::now());
                    }
                }
                () = sleep_until(reconnect_at) => {
                    checking = Some(self.check_offer(sent_hello, true, with_providers));
                }
                message = stream.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        if text.as_str() == PONG {
                            last_pong = Instant::now();
                            continue;
                        }
                        self.inbound(&text);
                    }
                    Some(Ok(Message::Close(frame))) => {
                        return close_exit(frame.as_ref(), &self.last, &self.coordinator);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return Exit::Retry { at_least: None, welcomed: true },
                },
            }
        }
    }

    /// Start an offer check aside; `confirming` marks the one after the debounce, and
    /// `with_providers` also compares provider availability (see [`PROVIDER_CHECK`]).
    fn check_offer(&self, sent_hello: &str, confirming: bool, with_providers: bool) -> Check {
        Check(
            tokio::spawn(offer_changed(
                self.host.clone(),
                Arc::clone(&self.offer),
                self.config.runtime_id().to_owned(),
                sent_hello.to_owned(),
                with_providers,
            )),
            confirming,
        )
    }

    fn inbound(&self, text: &str) {
        match wire::decode(text) {
            Ok(Incoming::Frame(frame)) => {
                let _ = self.coordinator.send(Input::Frame(frame));
            }
            Ok(Incoming::Malformed { kind, run_id }) => {
                let _ = self.coordinator.send(Input::Malformed { kind, run_id });
            }
            Ok(Incoming::Pong) => {}
            Err(_) => tracing::warn!("ignored a frame from Bonsai whose head is not JSON"),
        }
    }
}

/// Let a closing connection's writer finish, but never wait on a dead peer for long.
async fn drain_writer(mut writer: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(WRITER_DRAIN, &mut writer)
        .await
        .is_err()
    {
        writer.abort();
        tracing::warn!("the runtime connection's writer did not finish in time; abandoned it");
    }
}

/// A running offer check (and whether it confirms an earlier one), stopped when the
/// connection ends.
struct Check(tokio::task::JoinHandle<bool>, bool);

impl Drop for Check {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn finished(check: Option<&mut Check>) -> (bool, bool) {
    match check {
        Some(check) => ((&mut check.0).await.unwrap_or(false), check.1),
        None => std::future::pending().await,
    }
}

/// Whether the announced projects, or with `with_providers` the available providers, differ
/// from the last hello. The shared offer is left alone: a reconnect rediscovers everything,
/// and a change that does not last must not make dispatches to a briefly missing project fail.
async fn offer_changed(
    host: Host,
    offer: Arc<RwLock<Offer>>,
    runtime_id: String,
    sent_hello: String,
    with_providers: bool,
) -> bool {
    let available = if with_providers {
        hello::execute_retrying(
            host.executor.as_ref(),
            "provider.available.list.request",
            serde_json::json!({}),
        )
        .await
        .ok()
    } else {
        None
    };
    if let Some(available) = available {
        let now: Vec<&str> = hello::PROVIDERS
            .iter()
            .copied()
            .filter(|id| {
                available["providers"].as_array().is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| entry["provider"] == *id && entry["available"] == true)
                })
            })
            .collect();
        let announced: Vec<String> = offer
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect();
        if now != announced {
            return true;
        }
    }
    let Ok(projects) = host.projects.list().await else {
        return false;
    };
    let mut current = offer
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    current.projects = hello::announce(projects);
    let frame = current.hello_frame(&runtime_id).map(|(frame, _)| frame);
    frame.as_deref() != Some(sent_hello.as_str())
}

fn close_exit(
    frame: Option<&CloseFrame>,
    last: &Arc<Mutex<LastFrame>>,
    coordinator: &mpsc::UnboundedSender<Input>,
) -> Exit {
    let code = frame.map_or(1006, |frame| u16::from(frame.code));
    if code == 4400 {
        let last = last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        tracing::error!(kind = last.kind, run_id = ?last.run_id, range = ?last.range, "Bonsai closed the runtime connection (4400): the last frame was rejected");
        if let Some(rejected) = rejected_events(&last) {
            let _ = coordinator.send(rejected);
        }
    } else {
        tracing::info!(code, "Bonsai closed the runtime connection");
    }
    after_close(code)
}

/// The log range to quarantine after a `4400`; only `session.events` frames carry events.
#[must_use]
pub(crate) fn rejected_events(last: &LastFrame) -> Option<Input> {
    if last.kind != "session.events" {
        return None;
    }
    let (from, to) = last.range?;
    let to = u64::try_from(to).ok()?;
    Some(Input::Rejected {
        run_id: last.run_id.clone()?,
        epoch: last.epoch.clone()?,
        from,
        to,
    })
}

/// Close frame for an orderly shutdown.
#[must_use]
pub fn close_frame(code: u16) -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    }))
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
