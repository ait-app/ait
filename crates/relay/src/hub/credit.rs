//! Session flow control (`ait-hub-v1` feature `credit`).
//!
//! A session is flow-controlled when its `open` carried the opener's window `w`.
//! Each side may send `msg` payload bytes up to the peer's latest cumulative
//! credit `c` plus the peer's window, and grants credit for what it delivered.

use tokio::sync::watch;

/// This Host's receive window, answered in `accept.w`. The protocol maximum lets
/// any single message up to the payload limit fit the window.
pub(super) const WINDOW: u32 = 1024 * 1024;

/// Sending side: waits for the peer's credit before admitting more bytes.
#[derive(Debug)]
pub(super) struct Outgoing {
    peer: watch::Receiver<u64>,
    window: u64,
    sent: u64,
}

impl Outgoing {
    /// Track sending against `window`, with credit updates published on `peer`.
    pub(super) fn new(peer: watch::Receiver<u64>, window: u32) -> Self {
        Self {
            peer,
            window: u64::from(window),
            sent: 0,
        }
    }

    /// Wait until the peer window admits `length` more payload bytes, then count them.
    ///
    /// # Errors
    /// Returns `overflow` for a message larger than the whole peer window, which no
    /// credit could ever admit, and `closed` once the session's route is gone.
    pub(super) async fn admit(&mut self, length: usize) -> Result<(), &'static str> {
        let length = length as u64;
        if length > self.window {
            return Err("overflow");
        }
        let sent = self.sent + length;
        let window = self.window;
        self.peer
            .wait_for(|credit| sent <= credit.saturating_add(window))
            .await
            .map_err(|_| "closed")?;
        self.sent = sent;
        Ok(())
    }
}

/// Receiving side: counts locally delivered bytes and decides when to grant credit.
#[derive(Debug, Default)]
pub(super) struct Incoming {
    delivered: u64,
    granted: u64,
}

impl Incoming {
    /// Count `length` delivered bytes; returns the cumulative credit to announce
    /// once it grew by at least half of [`WINDOW`].
    pub(super) fn delivered(&mut self, length: usize) -> Option<u64> {
        self.delivered += length as u64;
        (self.delivered - self.granted >= u64::from(WINDOW / 2)).then(|| {
            self.granted = self.delivered;
            self.granted
        })
    }
}

#[cfg(test)]
mod tests;
