//! One hub session served in-process: a business connection to the local API,
//! or a file download.

use std::sync::Arc;
use std::time::Duration;

use model::share::Visitor;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use super::bridge::{Connection, Frame};
use super::credit;
use super::wire::{self, PayloadKind, SessionKind};
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
    /// Peer messages in link order; closed when the link drops the session.
    pub(super) inbound: mpsc::Receiver<Frame>,
    pub(super) output: mpsc::Sender<(Uuid, Output)>,
    /// Peer window and credit for a flow-controlled session.
    pub(super) credit: Option<credit::Outgoing>,
}

/// Serve session `kind` until either side closes, then report the closure.
/// `payload` is the `open` payload: the client hello, or the download request.
pub(super) async fn run(mut session: Session, kind: SessionKind, payload: Bytes) {
    let result = match kind {
        SessionKind::Rpc => bridge(&mut session, payload).await.map(|()| "closed"),
        SessionKind::Download => download::serve(&mut session, &payload)
            .await
            .map(|()| "done"),
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

/// Bridge a business session into the local API. A guest session is admitted
/// with its visitor; the API confines it to the shared resource and ends it
/// once the share is gone.
async fn bridge(session: &mut Session, hello: Bytes) -> Result<(), &'static str> {
    let text = std::str::from_utf8(&hello).map_err(|_| "invalid_hello")?;
    let _: ClientHello = protocol::decode(text).map_err(|_| "invalid_hello")?;
    let api = session.local.api.get().ok_or("unavailable")?;
    let Connection {
        sender,
        mut receiver,
    } = api.connect(session.visitor.as_ref()).await.map_err(|_| {
        if session.visitor.is_some() {
            "share_revoked"
        } else {
            "unavailable"
        }
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
    let (id, granting) = (session.id, session.credit.is_some());
    let mut outlet = Outlet {
        id,
        output: &session.output,
        credit: session.credit.as_mut(),
    };
    let to_peer = async {
        while let Some(frame) = receiver.recv().await {
            let (kind, body) = frame.into_parts();
            outlet.send(kind, body).await?;
        }
        Ok(())
    };
    let (inbound, output) = (&mut session.inbound, &session.output);
    let to_api = async {
        let mut delivered = credit::Incoming::default();
        // Closed when the link drops the session; returning drops `sender` and
        // with it the local connection.
        while let Some(frame) = inbound.recv().await {
            let length = frame.len();
            if sender.send(frame).await.is_err() {
                return Ok(());
            }
            if granting
                && let Some(credit) = delivered.delivered(length)
                && output.send((id, Output::Credit(credit))).await.is_err()
            {
                return Ok(());
            }
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
