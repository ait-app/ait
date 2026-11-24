//! Peer sessions this Host opens to call sibling Hosts. The link loop owns
//! them across physical reconnects: at most one session per target node,
//! opened on demand and shared by every call to that node, with request ids
//! counting up within the session.
//!
//! A request is held until the target accepts the session and its window
//! admits it, so a session the center refuses or ends before that has sent
//! nothing and its close code says why. Once a request was handed to the link,
//! any end of its session, or of the logical link, leaves its outcome unknown.
//! A session is closed as `done` after [`IDLE`] without requests, and before
//! the target accepts it once every caller waiting on it gave up.
//!
//! Each call carries its caller's time limit as a [`Progress`] shared with the
//! opener: at the limit the caller learns how far the call got, and the opener
//! never sends a request whose caller already gave up.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use model::hub::LinkState;
use model::peer::{Error, Request};
use serde_json::Value;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use uuid::Uuid;

use super::{HELLO, decode_hello, decode_reply, encode_request};
use crate::hub::link::Link;
use crate::hub::wire::{self, Outbound, PayloadKind, Target};
use crate::hub::{MAX_PEER_SESSIONS, credit};

/// A session without requests for this long is closed.
const IDLE: Duration = Duration::from_secs(60);
/// Calls on their way to the link loop.
const QUEUE: usize = 64;
/// Close code of a session this Host no longer needs.
const DONE: &str = "done";

type Reply = oneshot::Sender<Result<Value, Error>>;

/// One call on its way to the link loop.
#[derive(Debug)]
pub(in crate::hub) struct Call {
    node: Uuid,
    request: Request,
    reply: Reply,
    progress: Progress,
}

/// How far one call got. Shared by the caller and the opener as one atomic
/// byte, so the two agree on whether its request was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Stage {
    /// Waiting for the link loop, not yet on a session.
    Queued,
    /// Held on a session whose `open` the target has not accepted.
    Opening,
    /// Held on an accepted session until its window admits the request.
    Held,
    /// Handed to the link.
    Sent,
    /// Its caller gave up before it was sent; the opener drops it.
    GaveUp,
}

impl Stage {
    /// The stage stored as `raw`, which only ever holds a `Stage`.
    fn from_raw(raw: u8) -> Self {
        [
            Self::Queued,
            Self::Opening,
            Self::Held,
            Self::Sent,
            Self::GaveUp,
        ]
        .into_iter()
        .find(|stage| *stage as u8 == raw)
        .expect("progress only ever stores a stage")
    }

    /// The error of a call given up at its caller's time limit in this stage.
    ///
    /// # Arguments
    /// * `online` - Whether the link was connected at the limit.
    ///
    /// # Returns
    /// `Error::OutcomeUnknown` once the request was sent and `Error::Offline`
    /// while the target had not accepted its session. A call still waiting in
    /// the queue or for the target's window is `Error::Capacity` while the link
    /// is connected, and `Error::Unavailable` while it is not, since the link
    /// was then the reason.
    const fn at_limit(self, online: bool) -> Error {
        match self {
            Self::Sent => Error::OutcomeUnknown,
            Self::Opening => Error::Offline,
            Self::Queued | Self::Held if online => Error::Capacity,
            Self::Queued | Self::Held | Self::GaveUp => Error::Unavailable,
        }
    }

    /// The error of a call whose answer was dropped unsent: its request never
    /// left this Host unless it was handed to the link.
    const fn unanswered(self) -> Error {
        match self {
            Self::Sent => Error::OutcomeUnknown,
            Self::Queued | Self::Opening | Self::Held | Self::GaveUp => Error::Unavailable,
        }
    }
}

/// The [`Stage`] of one call.
#[derive(Clone, Debug)]
struct Progress(Arc<AtomicU8>);

impl Progress {
    /// A call waiting for the link loop.
    fn queued() -> Self {
        Self(Arc::new(AtomicU8::new(Stage::Queued as u8)))
    }

    fn stage(&self) -> Stage {
        Stage::from_raw(self.0.load(Ordering::Acquire))
    }

    /// Record that the call reached `stage`; it never moves back.
    ///
    /// # Returns
    /// `false` once the caller gave up; the opener must then drop the call.
    fn reach(&self, stage: Stage) -> bool {
        self.0
            .fetch_update(
                Ordering::AcqRel,
                Ordering::Acquire,
                |raw| match Stage::from_raw(raw) {
                    Stage::GaveUp => None,
                    current => Some(current.max(stage) as u8),
                },
            )
            .is_ok()
    }

    /// Give the call up at its caller's time limit unless its request was
    /// already sent.
    ///
    /// # Returns
    /// The stage the call had reached: `Stage::Sent`, or the stage it was
    /// given up in.
    fn give_up(&self) -> Stage {
        let previous = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |raw| {
                (Stage::from_raw(raw) != Stage::Sent).then_some(Stage::GaveUp as u8)
            });
        Stage::from_raw(previous.unwrap_or_else(|sent| sent))
    }
}

/// Hands calls to the link loop that owns the peer sessions.
#[derive(Clone, Debug)]
pub(in crate::hub) struct Caller {
    queue: mpsc::Sender<Call>,
    /// The link's state; a call given up while it is not connected waited for the link.
    link: watch::Receiver<LinkState>,
}

impl Caller {
    /// Send `request` to the Host on `node` and wait for its reply, at most
    /// `limit`. Dropping the future gives up the call too; if its session was
    /// not accepted yet, nothing was sent.
    ///
    /// # Errors
    /// Returns what [`Opener`] answered; `Error::Unavailable` once the link
    /// ended before the request was sent, and `Error::OutcomeUnknown` once it
    /// ended after; at the limit, the error [`Stage::at_limit`] gives.
    pub(in crate::hub) async fn call(
        &self,
        node: Uuid,
        request: Request,
        limit: Duration,
    ) -> Result<Value, Error> {
        let deadline = Instant::now() + limit;
        let progress = Progress::queued();
        let (reply, mut outcome) = oneshot::channel();
        let call = Call {
            node,
            request,
            reply,
            progress: progress.clone(),
        };
        match timeout_at(deadline, self.queue.send(call)).await {
            Err(_) => return Err(Stage::Queued.at_limit(self.online())),
            Ok(Err(_)) => return Err(Error::Unavailable),
            Ok(Ok(())) => {}
        }
        // The opener answers every call it took, even when the link ends; an
        // answer dropped unsent is decided by how far the call got.
        match timeout_at(deadline, &mut outcome).await {
            Ok(answer) => answer.unwrap_or_else(|_| Err(progress.stage().unanswered())),
            // An answer that raced the limit still counts.
            Err(_) => match outcome.try_recv() {
                Ok(answer) => answer,
                Err(TryRecvError::Empty) => Err(progress.give_up().at_limit(self.online())),
                Err(TryRecvError::Closed) => Err(progress.stage().unanswered()),
            },
        }
    }

    fn online(&self) -> bool {
        self.link.borrow().online
    }
}

/// The peer sessions this Host opened on one link.
pub(in crate::hub) struct Opener {
    pub(in crate::hub) queue: mpsc::Receiver<Call>,
    /// Whether the current logical link enabled `peer`; a peer `open` on a
    /// link without it would end the link.
    enabled: bool,
    sessions: HashMap<Uuid, Opened>,
}

/// One session this Host opened.
struct Opened {
    node: Uuid,
    next_id: u64,
    accepted: bool,
    /// Requests not yet handed to the link, in call order.
    held: VecDeque<Held>,
    /// Requests handed to the link, by id.
    sent: HashMap<u64, Reply>,
    /// Flow control, when the target's `accept` carried its window.
    window: Option<Window>,
    /// When an accepted session last ran out of requests.
    idle_since: Option<Instant>,
}

struct Held {
    id: u64,
    body: Bytes,
    reply: Reply,
    progress: Progress,
}

/// Flow control of an accepted session in both directions.
struct Window {
    /// The target's receive window.
    size: u64,
    /// The target's latest cumulative credit.
    credit: u64,
    /// Request bytes sent.
    sent: u64,
    /// Reply bytes delivered here.
    incoming: credit::Incoming,
    /// Latest credit this Host announced, repeated after a resume.
    granted: u64,
}

impl Opener {
    /// An opener and the handle that hands it calls.
    ///
    /// # Arguments
    /// * `link` - The state of the link the opener runs on.
    pub(in crate::hub) fn channel(link: watch::Receiver<LinkState>) -> (Caller, Self) {
        let (queue, receiver) = mpsc::channel(QUEUE);
        (
            Caller { queue, link },
            Self {
                queue: receiver,
                enabled: false,
                sessions: HashMap::new(),
            },
        )
    }

    /// Apply a welcome. A fresh logical link ended every session.
    ///
    /// # Arguments
    /// * `fresh` - Whether the welcome started a new logical link.
    /// * `enabled` - Whether the link enabled `peer`.
    pub(in crate::hub) fn joined(&mut self, fresh: bool, enabled: bool) {
        if fresh {
            self.end_all();
        }
        self.enabled = enabled;
    }

    /// Whether `s` is a session this Host opened.
    pub(in crate::hub) fn owns(&self, s: Uuid) -> bool {
        self.sessions.contains_key(&s)
    }

    /// Hold one call's request on its target's session, opening the session
    /// when there is none, and send what the session admits. A call is
    /// answered at once when the link has no `peer`, the request exceeds the
    /// payload limit, or opening one more session would exceed
    /// [`MAX_PEER_SESSIONS`].
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn call(
        &mut self,
        call: Call,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<(), crate::Error> {
        let Call {
            node,
            request,
            reply,
            progress,
        } = call;
        if reply.is_closed() {
            return Ok(());
        }
        if !self.enabled {
            let _ = reply.send(Err(Error::Unavailable));
            return Ok(());
        }
        let existing = self.sessions.iter().find_map(|(s, session)| {
            (session.node == node).then_some((*s, session.next_id, session.accepted))
        });
        let id = existing.map_or(1, |(_, id, _)| id);
        let body = encode_request(id, request);
        if body.len() > wire::MAX_PAYLOAD {
            let _ = reply.send(Err(Error::RequestTooLarge));
            return Ok(());
        }
        let (found, stage) = match existing {
            Some((s, _, true)) => (Some(s), Stage::Held),
            Some((s, _, false)) => (Some(s), Stage::Opening),
            None if self.sessions.len() >= MAX_PEER_SESSIONS => {
                let _ = reply.send(Err(Error::Capacity));
                return Ok(());
            }
            None => (None, Stage::Opening),
        };
        if !progress.reach(stage) {
            return Ok(());
        }
        let s = if let Some(s) = found {
            s
        } else {
            let s = Uuid::new_v4();
            // Opened with the largest window, so any reply within the
            // payload limit can arrive.
            let to = Target { node };
            let w = Some(credit::WINDOW);
            push(
                link,
                frames,
                |seq| Outbound::Open {
                    seq,
                    s,
                    to,
                    k: wire::PEER,
                    w,
                },
                HELLO,
            )?;
            s
        };
        let session = self.sessions.entry(s).or_insert_with(|| Opened::new(node));
        session.next_id = id + 1;
        session.held.push_back(Held {
            id,
            body,
            reply,
            progress,
        });
        session.send(s, link, frames)
    }

    /// Apply the target's `accept` of session `s`; returns whether this Host opened it.
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn accepted(
        &mut self,
        s: Uuid,
        window: Option<u32>,
        hello: &[u8],
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<bool, crate::Error> {
        let Some(session) = self.sessions.get_mut(&s) else {
            return Ok(false);
        };
        if session.accepted {
            return Ok(true);
        }
        if decode_hello(hello).is_err() {
            // The target speaks another version; nothing was sent.
            self.close(s, "unsupported", &Error::Unsupported, link, frames)?;
            return Ok(true);
        }
        session.accepted = true;
        session.window = window.map(Window::new);
        // From here a request still held waits for the window, not the target.
        for held in &session.held {
            held.progress.reach(Stage::Held);
        }
        session.send(s, link, frames)?;
        Ok(true)
    }

    /// Deliver one reply on session `s`, and credit it when the session is
    /// flow-controlled; returns whether this Host opened the session. A message
    /// that is not a reply ends the session as `invalid_message`.
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn message(
        &mut self,
        s: Uuid,
        kind: PayloadKind,
        payload: &[u8],
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<bool, crate::Error> {
        let Some(session) = self.sessions.get_mut(&s) else {
            return Ok(false);
        };
        let reply = match kind {
            PayloadKind::Text => decode_reply(payload).ok(),
            PayloadKind::Binary => None,
        };
        let Some((id, outcome)) = reply else {
            self.close(s, "invalid_message", &Error::Unavailable, link, frames)?;
            return Ok(true);
        };
        // A reply whose caller gave up has no one to reach.
        if let Some(reply) = session.sent.remove(&id) {
            let _ = reply.send(outcome);
        }
        if let Some(window) = session.window.as_mut()
            && let Some(c) = window.incoming.delivered(payload.len())
        {
            window.granted = c;
            frames.push(Message::Binary(wire::encode(
                &Outbound::Credit { s, c },
                b"",
            )?));
        }
        session.settle();
        Ok(true)
    }

    /// Record the target's cumulative credit for session `s` and send what it
    /// now admits; returns whether this Host opened the session.
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn credit(
        &mut self,
        s: Uuid,
        credit: u64,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<bool, crate::Error> {
        let Some(session) = self.sessions.get_mut(&s) else {
            return Ok(false);
        };
        if let Some(window) = session.window.as_mut() {
            window.credit = window.credit.max(credit);
        }
        session.send(s, link, frames)?;
        Ok(true)
    }

    /// End session `s`, which the center or the target closed with `code`;
    /// returns whether this Host opened it. Requests not yet sent fail with the
    /// reason `code` gives; those sent have an unknown outcome.
    pub(in crate::hub) fn closed(&mut self, s: Uuid, code: Option<&str>) -> bool {
        let Some(session) = self.sessions.remove(&s) else {
            return false;
        };
        session.end(&refusal(code));
        true
    }

    /// Forget calls whose callers gave up, and close the sessions not yet
    /// accepted that no caller waits for any more.
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn prune(
        &mut self,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<(), crate::Error> {
        let mut abandoned = Vec::new();
        self.sessions.retain(|s, session| {
            session.held.retain(|held| !held.reply.is_closed());
            session.sent.retain(|_, reply| !reply.is_closed());
            session.settle();
            let waited_for = session.accepted || !session.held.is_empty();
            if !waited_for {
                abandoned.push(*s);
            }
            waited_for
        });
        for s in abandoned {
            push(
                link,
                frames,
                |seq| Outbound::Close { seq, s, code: DONE },
                b"",
            )?;
        }
        Ok(())
    }

    /// When the first idle session is due to close.
    pub(in crate::hub) fn next_idle(&self) -> Option<Instant> {
        self.sessions
            .values()
            .filter_map(|session| session.idle_since)
            .min()
            .map(|since| since + IDLE)
    }

    /// Close every session idle for [`IDLE`] as `done`.
    ///
    /// # Errors
    /// Propagates frame encoding failures.
    pub(in crate::hub) fn expire(
        &mut self,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<(), crate::Error> {
        let now = Instant::now();
        let mut expired = Vec::new();
        self.sessions.retain(|s, session| {
            let due = session.idle_since.is_some_and(|since| since + IDLE <= now);
            if due {
                expired.push(*s);
            }
            !due
        });
        for s in expired {
            push(
                link,
                frames,
                |seq| Outbound::Close { seq, s, code: DONE },
                b"",
            )?;
        }
        Ok(())
    }

    /// Latest credit this Host announced on each session, repeated once after a resume.
    pub(in crate::hub) fn credits(&self) -> impl Iterator<Item = (Uuid, u64)> + '_ {
        self.sessions.iter().filter_map(|(s, session)| {
            let granted = session.window.as_ref()?.granted;
            (granted > 0).then_some((*s, granted))
        })
    }

    /// Close session `s` with `code`; its requests not yet sent fail with `unsent`.
    fn close(
        &mut self,
        s: Uuid,
        code: &'static str,
        unsent: &Error,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<(), crate::Error> {
        if let Some(session) = self.sessions.remove(&s) {
            session.end(unsent);
        }
        push(link, frames, |seq| Outbound::Close { seq, s, code }, b"")
    }

    /// End every session with the logical link.
    fn end_all(&mut self) {
        for (_, session) in self.sessions.drain() {
            session.end(&Error::Unavailable);
        }
    }
}

impl Drop for Opener {
    /// The link ended for good: answer every call still waiting.
    fn drop(&mut self) {
        self.end_all();
    }
}

impl Opened {
    fn new(node: Uuid) -> Self {
        Self {
            node,
            next_id: 1,
            accepted: false,
            held: VecDeque::new(),
            sent: HashMap::new(),
            window: None,
            idle_since: None,
        }
    }

    /// Hand the link every held request the session admits: none before the
    /// target accepts, then in call order within the target's window. A request
    /// larger than the whole window fails unsent as `Error::RequestTooLarge`.
    fn send(
        &mut self,
        s: Uuid,
        link: &mut Link,
        frames: &mut Vec<Message>,
    ) -> Result<(), crate::Error> {
        while self.accepted
            && let Some(held) = self.held.pop_front()
        {
            let length = held.body.len() as u64;
            if let Some(window) = &self.window {
                if length > window.size {
                    let _ = held.reply.send(Err(Error::RequestTooLarge));
                    continue;
                }
                if window.sent + length > window.credit.saturating_add(window.size) {
                    self.held.push_front(held);
                    break;
                }
            }
            if held.reply.is_closed() || !held.progress.reach(Stage::Sent) {
                continue;
            }
            let pt = PayloadKind::Text;
            push(link, frames, |seq| Outbound::Msg { seq, s, pt }, &held.body)?;
            if let Some(window) = self.window.as_mut() {
                window.sent += length;
            }
            self.sent.insert(held.id, held.reply);
        }
        self.settle();
        Ok(())
    }

    /// Start or stop the idle clock: an accepted session without requests is idle.
    fn settle(&mut self) {
        if self.accepted && self.held.is_empty() && self.sent.is_empty() {
            self.idle_since.get_or_insert_with(Instant::now);
        } else {
            self.idle_since = None;
        }
    }

    /// Answer every call of the ended session: those not yet sent with
    /// `unsent`, those sent with an unknown outcome.
    fn end(self, unsent: &Error) {
        for held in self.held {
            let _ = held.reply.send(Err(unsent.clone()));
        }
        for (_, reply) in self.sent {
            let _ = reply.send(Err(Error::OutcomeUnknown));
        }
    }
}

impl Window {
    fn new(size: u32) -> Self {
        Self {
            size: u64::from(size),
            credit: 0,
            sent: 0,
            incoming: credit::Incoming::default(),
            granted: 0,
        }
    }
}

/// Why a session closed before its requests were sent, from its close code. A target that went
/// away (`peer_gone`) is offline like one never found; a replaced session and any other code
/// leave the Hub as the cause.
fn refusal(code: Option<&str>) -> Error {
    match code {
        Some("unknown_target" | "peer_gone") => Error::Offline,
        Some("unsupported") => Error::Unsupported,
        Some("peer_disabled") => Error::NotAccepting,
        Some("capacity") => Error::Capacity,
        _ => Error::Unavailable,
    }
}

/// Number one sequenced frame on `link` and queue it for the connection.
fn push(
    link: &mut Link,
    frames: &mut Vec<Message>,
    header: impl FnOnce(u64) -> Outbound,
    payload: &[u8],
) -> Result<(), crate::Error> {
    frames.push(Message::Binary(link.push(header, payload)?));
    Ok(())
}

#[cfg(test)]
mod tests;
