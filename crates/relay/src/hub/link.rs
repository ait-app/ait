//! Sequence, acknowledgement and replay state of one hub link across reconnects.

use std::collections::VecDeque;

use secrecy::SecretString;
use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use super::wire::{self, Outbound, Resume};
use crate::Error;

/// Unacknowledged outbound bytes kept for replay; session reads pause beyond it.
const MAX_UNACKED: usize = 8 * 1024 * 1024;

/// What a welcome means for the sessions carried by this link.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Joined {
    /// Same link: replay these frames; sessions continue.
    Resumed(Vec<Bytes>),
    /// New link: every session from an earlier link is gone.
    Fresh,
}

#[derive(Debug, Default)]
pub(super) struct Link {
    id: Option<Uuid>,
    token: Option<SecretString>,
    recv: u64,
    next_seq: u64,
    unacked: VecDeque<(u64, Bytes)>,
    unacked_bytes: usize,
    ack_due: bool,
}

impl Link {
    /// Center-assigned identifier once a welcome was received.
    pub(super) const fn id(&self) -> Option<Uuid> {
        self.id
    }

    /// Resume credential for reconnecting without a fresh ticket.
    pub(super) const fn token(&self) -> Option<&SecretString> {
        self.token.as_ref()
    }

    /// Build the hello for the next physical connection.
    ///
    /// # Errors
    /// Propagates encoding failures.
    pub(super) fn hello(&self) -> Result<Bytes, Error> {
        let resume = self.id.map(|link| Resume {
            link,
            recv: self.recv,
        });
        wire::encode(
            &Outbound::Hello {
                v: wire::VERSION,
                resume,
            },
            b"",
        )
    }

    /// Apply a welcome. On resume, drop what the center already holds and return
    /// the rest for replay; otherwise restart both sequence spaces.
    ///
    /// # Errors
    /// Returns `Error::Protocol` when the center claims frames this link never sent.
    pub(super) fn join(
        &mut self,
        link: Uuid,
        resumed: bool,
        received: u64,
        token: Option<String>,
    ) -> Result<Joined, Error> {
        if resumed && self.id == Some(link) {
            self.acked(received)?;
            return Ok(Joined::Resumed(
                self.unacked
                    .iter()
                    .map(|(_, frame)| frame.clone())
                    .collect(),
            ));
        }
        let token = token.ok_or(Error::Protocol)?;
        *self = Self {
            id: Some(link),
            token: Some(SecretString::from(token)),
            ..Self::default()
        };
        Ok(Joined::Fresh)
    }

    /// Whether session output can be admitted without exceeding the replay budget.
    pub(super) const fn has_room(&self) -> bool {
        self.unacked_bytes < MAX_UNACKED
    }

    /// Number and retain one sequenced frame; returns the bytes to write.
    ///
    /// # Errors
    /// Propagates encoding failures for oversized payloads.
    pub(super) fn push(
        &mut self,
        header: impl FnOnce(u64) -> Outbound,
        payload: &[u8],
    ) -> Result<Bytes, Error> {
        let seq = self.next_seq + 1;
        let frame = wire::encode(&header(seq), payload)?;
        self.next_seq = seq;
        self.unacked_bytes += frame.len();
        self.unacked.push_back((seq, frame.clone()));
        Ok(frame)
    }

    /// Release frames covered by a cumulative center acknowledgement.
    ///
    /// # Errors
    /// Returns `Error::Protocol` for an acknowledgement beyond the last sent frame.
    pub(super) fn acked(&mut self, acked: u64) -> Result<(), Error> {
        if acked > self.next_seq {
            return Err(Error::Protocol);
        }
        while let Some((_, frame)) = self.unacked.front().filter(|(seq, _)| *seq <= acked) {
            self.unacked_bytes -= frame.len();
            self.unacked.pop_front();
        }
        Ok(())
    }

    /// Record an inbound sequenced frame; returns `false` for a replayed duplicate.
    pub(super) const fn received(&mut self, seq: u64) -> bool {
        self.ack_due = true;
        if seq <= self.recv {
            return false;
        }
        self.recv = seq;
        true
    }

    /// Cumulative acknowledgement to send, if any frame arrived since the last one.
    ///
    /// # Errors
    /// Propagates encoding failures.
    pub(super) fn take_ack(&mut self) -> Result<Option<Bytes>, Error> {
        if !std::mem::take(&mut self.ack_due) {
            return Ok(None);
        }
        wire::encode(&Outbound::Ack { a: self.recv }, b"").map(Some)
    }
}

#[cfg(test)]
mod tests;
