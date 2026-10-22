//! Replay storage belongs to a logical link, not to a physical socket.

use std::collections::VecDeque;

use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use super::wire::{self, Outbound, Resume, Welcome};
use crate::Error;

pub(super) const BUFFER_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING: usize = 65_536;

struct Pending {
    seq: u64,
    session: Uuid,
    bytes: Bytes,
}

#[derive(Default)]
pub(super) struct Link {
    pub resume: Option<Resume>,
    pub token: Option<secrecy::SecretString>,
    node: Option<Uuid>,
    next: u64,
    acked: u64,
    sent: u64,
    bytes: usize,
    queue: VecDeque<Pending>,
}

impl Link {
    pub(super) fn welcome(&mut self, welcome: Welcome) -> Result<u64, Error> {
        if welcome.v != 1
            || welcome.link.is_nil()
            || welcome.node_id.is_nil()
            || welcome.grace_seconds == 0
            || welcome.grace_seconds > 120
        {
            return Err(Error::Protocol);
        }
        match self.resume {
            Some(resume)
                if !welcome.resumed
                    || welcome.link != resume.link
                    || self.node != Some(welcome.node_id) =>
            {
                return Err(Error::Protocol);
            }
            None if welcome.resumed || welcome.recv != 0 => return Err(Error::Protocol),
            _ => {}
        }
        if self.resume.is_none() {
            let token = welcome.resume_token.ok_or(Error::Protocol)?;
            if !super::valid_ticket(secrecy::ExposeSecret::expose_secret(&token)) {
                return Err(Error::Protocol);
            }
            self.token = Some(token);
            self.node = Some(welcome.node_id);
            self.resume = Some(Resume {
                link: welcome.link,
                recv: 0,
            });
        }
        self.ack(welcome.recv)?;
        self.sent = welcome.recv;
        Ok(welcome.grace_seconds)
    }

    pub(super) fn ack(&mut self, received: u64) -> Result<(), Error> {
        if received > self.sent || received < self.acked {
            return Err(Error::Protocol);
        }
        self.acked = received;
        while self
            .queue
            .front()
            .is_some_and(|frame| frame.seq <= received)
        {
            if let Some(frame) = self.queue.pop_front() {
                self.bytes -= frame.bytes.len();
            }
        }
        Ok(())
    }

    pub(super) fn receive(&mut self, seq: u64) -> Result<bool, Error> {
        let resume = self.resume.as_mut().ok_or(Error::Protocol)?;
        if seq == 0 {
            return Err(Error::Protocol);
        }
        if seq <= resume.recv {
            return Ok(false);
        }
        // Presence/coalescing and terminated sessions can leave gaps in center sequences.
        resume.recv = seq;
        Ok(true)
    }

    pub(super) fn push(
        &mut self,
        session: Uuid,
        payload: &[u8],
        header: impl FnOnce(u64) -> Outbound,
    ) -> Result<(), Error> {
        let seq = self.next.checked_add(1).ok_or(Error::Protocol)?;
        let header = header(seq);
        let essential = matches!(header, Outbound::Close { .. });
        let bytes = wire::encode(&header, payload)?;
        let limit = BUFFER_BYTES + if essential { 16 * wire::MAX_HEADER } else { 0 };
        if self.bytes + bytes.len() > limit || (!essential && self.queue.len() >= MAX_PENDING) {
            return Err(Error::Transport);
        }
        self.next = seq;
        self.bytes += bytes.len();
        self.queue.push_back(Pending {
            seq,
            session,
            bytes,
        });
        Ok(())
    }

    pub(super) fn close(&mut self, session: Uuid, code: &'static str) -> Result<(), Error> {
        self.queue.retain(|frame| {
            if frame.session == session {
                self.bytes -= frame.bytes.len();
                false
            } else {
                true
            }
        });
        self.push(session, &[], |seq| Outbound::Close {
            seq,
            s: session,
            code,
        })
    }

    pub(super) fn next(&self) -> Option<(u64, Bytes)> {
        let position = self.queue.partition_point(|frame| frame.seq <= self.sent);
        self.queue
            .get(position)
            .map(|frame| (frame.seq, frame.bytes.clone()))
    }

    pub(super) fn sent(&mut self, seq: u64) {
        self.sent = seq;
    }
}

#[cfg(test)]
mod tests;
