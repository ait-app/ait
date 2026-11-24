//! One hub session served in-process: a business connection to the local API,
//! a file download, or calls from a sibling Host.

use std::sync::Arc;
use std::time::Duration;

use model::peer::Identity;
use model::share::Visitor;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Bytes;
use tracing::Instrument;
use uuid::Uuid;

use super::bridge::{Admission, Connection, Frame, PeerRefusal};
use super::wire::{self, PayloadKind, SessionKind};
use super::{MAX_PEER_CALLS, credit, peer};
use crate::Local;
use crate::protocol::{self, ClientHello};

mod download;

/// The local API must answer the business hello promptly.
const FIRST_REPLY: Duration = Duration::from_secs(10);

/// Session output for the link, in session order.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Output {
    /// The session was admitted; carries the accept payload (the API's first reply).
    Accept(Bytes),
    Message(PayloadKind, Bytes),
    /// Cumulative inbound payload bytes delivered locally (flow-controlled sessions).
    Credit(u64),
    /// The session ended; the code is relayed to the access endpoint.
    Closed(&'static str),
}

/// Everything one session task owns.
#[derive(Debug)]
pub(super) struct Session {
    pub(super) id: Uuid,
    pub(super) local: Arc<Local>,
    pub(super) visitor: Option<Visitor>,
    /// Whether the center opened this session for automation.
    pub(super) automation: bool,
    /// The calling Host of a peer session.
    pub(super) peer: Option<Identity>,
    /// Peer messages in link order; closed when the link drops the session.
    pub(super) inbound: mpsc::Receiver<Frame>,
    pub(super) output: mpsc::Sender<(Uuid, Output)>,
    /// Peer window and credit for a flow-controlled session.
    pub(super) credit: Option<credit::Outgoing>,
}

/// Serve session `kind` until either side closes, then report the closure.
/// `payload` is the `open` payload: the client hello, the download request, or
/// the peer version.
pub(super) async fn run(mut session: Session, kind: SessionKind, payload: Bytes) {
    let result = match kind {
        SessionKind::Rpc => bridge(&mut session, payload).await.map(|()| "closed"),
        SessionKind::Download => download::serve(&mut session, &payload)
            .await
            .map(|()| "done"),
        SessionKind::Peer => serve_peer(&mut session, &payload).await.map(|()| "closed"),
        SessionKind::Unsupported => Err("unsupported"),
    };
    let code = result.unwrap_or_else(|code| code);
    let _ = session
        .output
        .send((session.id, Output::Closed(code)))
        .await;
}

/// Messages to the peer, admitted by its window when the session is flow-controlled.
struct Outlet<'a> {
    id: Uuid,
    output: &'a mpsc::Sender<(Uuid, Output)>,
    credit: Option<&'a mut credit::Outgoing>,
}

impl Outlet<'_> {
    /// Forward one message once the peer window admits it.
    ///
    /// # Errors
    /// Returns `overflow` for a message no frame or window could carry, ending only
    /// this session, and `closed` once the link dropped the session.
    async fn send(&mut self, kind: PayloadKind, body: Bytes) -> Result<(), &'static str> {
        // Checked here for sessions without credit too: the link cannot encode it,
        // and failing there would end every session on the link.
        if body.len() > wire::MAX_PAYLOAD {
            return Err("overflow");
        }
        if let Some(credit) = self.credit.as_deref_mut() {
            credit.admit(body.len()).await?;
        }
        self.output
            .send((self.id, Output::Message(kind, body)))
            .await
            .map_err(|_| "closed")
    }
}

/// Credit for peer messages delivered locally, announced when the session is
/// flow-controlled.
struct Inlet<'a> {
    id: Uuid,
    output: &'a mpsc::Sender<(Uuid, Output)>,
    delivered: Option<credit::Incoming>,
}

impl Inlet<'_> {
    /// Count one delivered message of `length` payload bytes and announce the
    /// cumulative credit.
    ///
    /// # Errors
    /// Returns `closed` once the link dropped the session.
    async fn delivered(&mut self, length: usize) -> Result<(), &'static str> {
        let Some(credit) = self
            .delivered
            .as_mut()
            .and_then(|delivered| delivered.delivered(length))
        else {
            return Ok(());
        };
        self.output
            .send((self.id, Output::Credit(credit)))
            .await
            .map_err(|_| "closed")
    }
}

impl Session {
    /// Split the session into peer messages in, messages to the peer, and the
    /// credit for messages delivered locally.
    fn channels(&mut self) -> (&mut mpsc::Receiver<Frame>, Outlet<'_>, Inlet<'_>) {
        let outlet = Outlet {
            id: self.id,
            output: &self.output,
            credit: self.credit.as_mut(),
        };
        let inlet = Inlet {
            id: self.id,
            output: &self.output,
            delivered: outlet.credit.is_some().then(credit::Incoming::default),
        };
        (&mut self.inbound, outlet, inlet)
    }
}

/// The authority `session` runs with in the local API.
fn admission(session: &Session) -> Admission<'_> {
    match (&session.visitor, session.automation) {
        (Some(visitor), _) => Admission::Guest(visitor),
        (None, true) => Admission::Automation,
        (None, false) => Admission::Owner,
    }
}

/// Bridge a business session into the local API. A guest session is admitted
/// with its visitor; the API confines it to the shared resource and ends it
/// once the share is gone. An automation session is admitted only while this
/// Host has automation enabled, and confined to the automation policy.
async fn bridge(session: &mut Session, hello: Bytes) -> Result<(), &'static str> {
    let text = std::str::from_utf8(&hello).map_err(|_| "invalid_hello")?;
    let _: ClientHello = protocol::decode(text).map_err(|_| "invalid_hello")?;
    let api = session.local.api.get().ok_or("unavailable")?;
    let Connection {
        sender,
        mut receiver,
    } = api
        .connect(admission(session))
        .await
        .map_err(|_| match admission(session) {
            Admission::Guest(_) => "share_revoked",
            Admission::Automation => "forbidden",
            Admission::Owner => "unavailable",
        })?;
    sender
        .send(Frame::Text(hello))
        .await
        .map_err(|_| "unavailable")?;
    let accept = match timeout(FIRST_REPLY, receiver.recv()).await {
        Ok(Some(Frame::Text(info))) => info,
        Ok(Some(Frame::Binary(_)) | None) => return Err("unavailable"),
        Err(_) => return Err("timeout"),
    };
    if session
        .output
        .send((session.id, Output::Accept(accept)))
        .await
        .is_err()
    {
        return Ok(());
    }
    let (inbound, mut outlet, mut inlet) = session.channels();
    let to_peer = async {
        while let Some(frame) = receiver.recv().await {
            let (kind, body) = frame.into_parts();
            outlet.send(kind, body).await?;
        }
        Ok(())
    };
    let to_api = async {
        // Closed when the link drops the session; returning drops `sender` and
        // with it the local connection.
        while let Some(frame) = inbound.recv().await {
            let length = frame.len();
            if sender.send(frame).await.is_err() {
                return Ok(());
            }
            inlet.delivered(length).await?;
        }
        Ok(())
    };
    tokio::select! {
        result = to_peer => result,
        result = to_api => result,
    }
}

/// Serve calls from the sibling Host that opened a peer session. Each request
/// runs in its own task and is answered when it completes, so replies may
/// leave out of order. At most [`MAX_PEER_CALLS`] calls run or wait to be
/// answered at once; a request beyond that is neither read nor credited until
/// one is answered, so the opener's window and the session queue bound the
/// rest. The session ends when the center closes it, without cancelling calls
/// that already run, or after answering a call this Host no longer accepts.
///
/// # Arguments
/// * `session` - The session, carrying the calling Host the center named.
/// * `hello` - The `open` payload, which must declare the peer version.
///
/// # Errors
/// Returns the close code: `unsupported` for another version or a session
/// without a calling Host, `unavailable` without a local API,
/// `invalid_message` for a message that is not a request, `peer_disabled`
/// after answering a call this Host no longer accepts, and `overflow` for a
/// reply the opener's window cannot carry.
async fn serve_peer(session: &mut Session, hello: &[u8]) -> Result<(), &'static str> {
    peer::decode_hello(hello).map_err(|_| "unsupported")?;
    // `Sessions::open` admits no peer session without one; checked again here.
    let from = Arc::new(session.peer.take().ok_or("unsupported")?);
    let api = session.local.api.get().ok_or("unavailable")?.clone();
    let accept = Output::Accept(Bytes::from_static(peer::HELLO));
    if session.output.send((session.id, accept)).await.is_err() {
        return Ok(());
    }
    let calls = Arc::new(Semaphore::new(MAX_PEER_CALLS));
    // Each reply carries its call's permit, so it holds a slot until sent.
    let (replies_tx, mut replies) = mpsc::channel(MAX_PEER_CALLS);
    let (inbound, mut outlet, mut inlet) = session.channels();
    let to_peer = async {
        while let Some((request, outcome, _permit)) = replies.recv().await {
            let disabled = matches!(outcome, Err(PeerRefusal::Disabled));
            let reply = peer::encode_reply(request, outcome.map_err(peer::refusal));
            outlet.send(PayloadKind::Text, reply).await?;
            if disabled {
                return Err("peer_disabled");
            }
        }
        Ok(())
    };
    let to_api = async {
        // Closed when the link drops the session.
        while let Some(frame) = inbound.recv().await {
            let length = frame.len();
            let Frame::Text(text) = frame else {
                return Err("invalid_message");
            };
            let (request, call) = peer::decode_request(&text).map_err(|_| "invalid_message")?;
            let permit = calls.clone().acquire_owned().await.map_err(|_| "closed")?;
            let (api, from, replies) = (api.clone(), from.clone(), replies_tx.clone());
            tokio::spawn(
                async move {
                    let outcome = api.peer(&from, call).await;
                    let _ = replies.send((request, outcome, permit)).await;
                }
                .in_current_span(),
            );
            inlet.delivered(length).await?;
        }
        Ok(())
    };
    tokio::select! {
        result = to_peer => result,
        result = to_api => result,
    }
}

/// Convert a hub payload into the business message it carried.
///
/// # Errors
/// Returns `invalid_message` for text payloads that are not UTF-8.
pub(super) fn message(kind: PayloadKind, payload: Bytes) -> Result<Frame, &'static str> {
    match kind {
        PayloadKind::Text => {
            if std::str::from_utf8(&payload).is_err() {
                return Err("invalid_message");
            }
            Ok(Frame::Text(payload))
        }
        PayloadKind::Binary => Ok(Frame::Binary(payload)),
    }
}

#[cfg(test)]
mod tests;
