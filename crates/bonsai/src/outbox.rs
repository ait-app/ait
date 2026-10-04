//! The single serial writer of one runtime connection.
//!
//! Every frame goes through here, so a subscription answer is atomic: this run's live frames
//! wait behind it. Live frames of all runs share one budget per connection (about 15 frames
//! per second, leaving room under the Hub's 20): runs whose events are due take turns, one
//! slot each, and events keep accumulating in the log until their run's turn comes, so none is
//! lost. Status frames and subscription answers have their own buckets; a status the Hub
//! asked for (`run.query`, `run.cancel`) is free, as on the Hub. The heartbeat `ping` is sent
//! here too, between frames when a long answer is being written, so it never waits behind one.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::store::Store;
use crate::wire::{self, Batch, Cursor, Status, Unavailable};

/// Live frames per second for all runs together.
pub const LIVE_FRAMES_PER_SECOND: u32 = 15;
/// Heartbeat interval.
pub const HEARTBEAT: Duration = Duration::from_secs(25);
// The Hub closes the connection on the first status or answer over its limits (2/s burst 20,
// 200/s burst 2000), so these keep a quarter of headroom, as the live budget does.
const STATUS_PER_SECOND: f64 = 1.5;
const STATUS_BURST: f64 = 15.0;
const ANSWER_PER_SECOND: f64 = 150.0;
const ANSWER_BURST: f64 = 1500.0;

/// Something to send on the current connection.
#[derive(Debug, Clone, PartialEq)]
pub enum Outgoing {
    /// A status frame.
    Status(Status),
    /// The Hub sent `run.query` or `run.cancel` for this run: its next status is not counted.
    Credit {
        /// Run asked about.
        run_id: String,
    },
    /// New events were appended to a run's log.
    Live {
        /// Run with new events.
        run_id: String,
    },
    /// Send a run's unsent events now (session closing).
    Flush {
        /// Run to flush.
        run_id: String,
    },
    /// Answer a subscription from the log.
    Answer {
        /// Run to replay.
        run_id: String,
        /// Subscription ID.
        sub: String,
        /// Subscriber's cursor.
        after: Option<Cursor>,
    },
    /// Tell everyone an input or answer cannot be handled (`session.unavailable{ref}`).
    Unavailable {
        /// Run named by the frame.
        run_id: String,
        /// Input or ask ID.
        reference: String,
    },
    /// Close the connection with this code and stop writing.
    Close(u16),
    /// A run's log restarted in a new epoch; broadcast an empty frame from zero.
    Epoch {
        /// Run whose log rotated.
        run_id: String,
        /// The new epoch.
        epoch: String,
        /// End of the rebuilt log; later events go out live.
        next: u64,
    },
}

/// What the writer last put on the wire, for `4400` handling.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LastFrame {
    /// Frame type.
    pub kind: String,
    /// Run it concerned.
    pub run_id: Option<String>,
    /// Epoch of a `session.events` frame.
    pub epoch: Option<String>,
    /// First and last sequence numbers of a `session.events` frame.
    pub range: Option<(u64, i64)>,
}

/// Cloneable handle that reaches the writer of the current connection, if any.
///
/// Without a connection, frames are dropped: statuses are re-derived when the Hub asks, and
/// events stay in the log for the next subscription.
#[derive(Debug, Clone, Default)]
pub struct Outbox {
    sender: Arc<Mutex<Option<mpsc::UnboundedSender<Outgoing>>>>,
}

impl Outbox {
    /// Queue a frame for the current connection.
    pub fn send(&self, outgoing: Outgoing) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(sender) = sender.as_ref() {
            let _ = sender.send(outgoing);
        }
    }

    /// Attach a new connection's writer queue.
    pub fn attach(&self, sender: mpsc::UnboundedSender<Outgoing>) {
        *self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
    }

    /// Detach the writer; later frames are dropped until the next connection.
    pub fn detach(&self) {
        *self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

/// Token bucket with a sustained rate and a burst.
#[derive(Debug, Clone)]
pub struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    /// A full bucket.
    #[must_use]
    pub fn new(rate: f64, burst: f64) -> Self {
        Self {
            rate,
            burst,
            tokens: burst,
            at: Instant::now(),
        }
    }

    /// How long until one token is available (zero when one is).
    #[must_use]
    pub fn wait(&mut self) -> Duration {
        self.refill();
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((1.0 - self.tokens) / self.rate)
        }
    }

    /// Spend one token (may go negative, which later waits make up).
    pub fn take(&mut self) {
        self.refill();
        self.tokens -= 1.0;
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.at = now;
    }
}

/// Where a run's live delivery stands on this connection.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pointer {
    epoch: String,
    next: u64,
}

/// Sink of encoded text messages (the WebSocket in production, a vector in tests).
pub trait Sink: Send {
    /// Send one text message.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` when the connection is gone.
    fn send(&mut self, text: String) -> crate::ports::BoxFuture<'_, Result<(), ()>>;

    /// Close the connection with a close code.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` when the connection is already gone.
    fn close(&mut self, code: u16) -> crate::ports::BoxFuture<'_, Result<(), ()>>;
}

/// Serial writer state for one connection.
pub struct Writer<S: Sink> {
    sink: S,
    store: Store,
    pointers: HashMap<String, Pointer>,
    due: VecDeque<String>,
    live_at: Instant,
    status: Bucket,
    statuses: VecDeque<Status>,
    credits: HashMap<String, u32>,
    answers: Bucket,
    ping_at: Instant,
    last: Arc<Mutex<LastFrame>>,
}

/// What woke the writer.
enum Wake {
    Queued(Outgoing),
    Live,
    Status,
    Ping,
}

impl<S: Sink> Writer<S> {
    /// Start a writer whose live delivery begins at every known run's current log end: events
    /// appended while disconnected are not resent live (subscribers replay them). Runs created
    /// later start from zero.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the runs cannot be read.
    pub fn new(
        sink: S,
        store: Store,
        last: Arc<Mutex<LastFrame>>,
    ) -> Result<Self, crate::store::StoreError> {
        let pointers = store
            .ends()?
            .into_iter()
            .map(|(run_id, epoch, next)| (run_id, Pointer { epoch, next }))
            .collect();
        Ok(Self {
            sink,
            store,
            pointers,
            due: VecDeque::new(),
            live_at: Instant::now(),
            status: Bucket::new(STATUS_PER_SECOND, STATUS_BURST),
            statuses: VecDeque::new(),
            credits: HashMap::new(),
            answers: Bucket::new(ANSWER_PER_SECOND, ANSWER_BURST),
            ping_at: Instant::now() + HEARTBEAT,
            last,
        })
    }

    /// Serve the queue until it closes or the connection fails.
    pub async fn run(mut self, mut queue: mpsc::UnboundedReceiver<Outgoing>) {
        loop {
            let next_live = (!self.due.is_empty()).then_some(self.live_at);
            let next_status =
                (!self.statuses.is_empty()).then(|| Instant::now() + self.status.wait());
            let wake = tokio::select! {
                outgoing = queue.recv() => match outgoing {
                    Some(outgoing) => Wake::Queued(outgoing),
                    None => return,
                },
                () = sleep_until(next_live) => Wake::Live,
                () = sleep_until(next_status) => Wake::Status,
                () = tokio::time::sleep_until(self.ping_at) => Wake::Ping,
            };
            let result = match wake {
                Wake::Queued(outgoing) => self.handle(outgoing).await,
                Wake::Live => self.drain_due().await,
                Wake::Status => self.drain_statuses().await,
                Wake::Ping => self.ping_if_due().await,
            };
            if result.is_err() {
                return;
            }
        }
    }

    async fn handle(&mut self, outgoing: Outgoing) -> Result<(), ()> {
        match outgoing {
            Outgoing::Status(status) => {
                let credit = self
                    .credits
                    .get_mut(&status.run_id)
                    .filter(|credit| **credit > 0);
                if let Some(credit) = credit {
                    // Asked for: free. Waiting statuses of this run that it supersedes go; a
                    // newer one that was waiting (the run moved on after the Hub asked) stays.
                    *credit -= 1;
                    self.statuses.retain(|waiting| {
                        waiting.run_id != status.run_id || waiting.status > status.status
                    });
                    return self.send_status(&status).await;
                }
                self.statuses.push_back(status);
                self.drain_statuses().await
            }
            Outgoing::Credit { run_id } => {
                *self.credits.entry(run_id).or_default() += 1;
                Ok(())
            }
            Outgoing::Live { run_id } => {
                if !self.due.contains(&run_id) {
                    self.due.push_back(run_id);
                }
                if Instant::now() >= self.live_at {
                    self.drain_due().await
                } else {
                    Ok(())
                }
            }
            Outgoing::Close(code) => {
                let _ = self.sink.close(code).await;
                Err(())
            }
            Outgoing::Flush { run_id } => {
                self.due.retain(|due| due != &run_id);
                self.flush(&run_id).await
            }
            Outgoing::Answer { run_id, sub, after } => self.answer(&run_id, &sub, after).await,
            Outgoing::Unavailable { run_id, reference } => {
                self.spend_live(1);
                let Ok(frame) = wire::unavailable(&run_id, Unavailable::Ref(&reference)) else {
                    return Ok(());
                };
                self.record("session.unavailable", Some(&run_id), None, None);
                self.sink.send(frame).await
            }
            Outgoing::Epoch {
                run_id,
                epoch,
                next,
            } => {
                self.pointers.insert(
                    run_id.clone(),
                    Pointer {
                        epoch: epoch.clone(),
                        next,
                    },
                );
                self.spend_live(1);
                let Ok(frame) = wire::new_epoch(&run_id, &epoch) else {
                    return Ok(());
                };
                self.record("session.events", Some(&run_id), Some(&epoch), Some((0, -1)));
                self.sink.send(frame).await?;
                if self.due.contains(&run_id) && Instant::now() >= self.live_at {
                    self.drain_due().await?;
                }
                Ok(())
            }
        }
    }

    /// Send waiting statuses while the status bucket allows; the rest wait for the run loop.
    async fn drain_statuses(&mut self) -> Result<(), ()> {
        while !self.statuses.is_empty() && self.status.wait().is_zero() {
            self.status.take();
            if let Some(status) = self.statuses.pop_front() {
                self.send_status(&status).await?;
            }
        }
        Ok(())
    }

    async fn send_status(&mut self, status: &Status) -> Result<(), ()> {
        let Ok(frame) = wire::status(status) else {
            tracing::error!("run status frame exceeds the protocol limit");
            return Ok(());
        };
        self.ping_if_due().await?;
        self.record("run.status", Some(&status.run_id), None, None);
        self.sink.send(frame).await
    }

    /// Send the heartbeat when it is due.
    async fn ping_if_due(&mut self) -> Result<(), ()> {
        if Instant::now() < self.ping_at {
            return Ok(());
        }
        self.ping_at = Instant::now() + HEARTBEAT;
        self.sink.send(wire::PING.to_owned()).await
    }

    /// Send the runs whose turn has come, one slot each.
    async fn drain_due(&mut self) -> Result<(), ()> {
        while Instant::now() >= self.live_at {
            let Some(run_id) = self.due.pop_front() else {
                break;
            };
            self.flush(&run_id).await?;
        }
        Ok(())
    }

    /// Send a run's unsent events as live frames, spending their slots.
    async fn flush(&mut self, run_id: &str) -> Result<(), ()> {
        let Ok(Some(run)) = self.store.run(run_id) else {
            return Ok(());
        };
        let pointer = self
            .pointers
            .entry(run_id.to_owned())
            .or_insert_with(|| Pointer {
                epoch: run.epoch.clone(),
                next: 0,
            });
        if pointer.epoch != run.epoch {
            // The rotation announced itself through `Epoch`; nothing older is sent live.
            *pointer = Pointer {
                epoch: run.epoch.clone(),
                next: run.next_seq,
            };
            return Ok(());
        }
        let from = pointer.next;
        if from >= run.next_seq {
            return Ok(());
        }
        pointer.next = run.next_seq;
        let Ok(events) = self.store.events(run_id, &run.epoch, from, run.next_seq) else {
            tracing::error!(run_id, "run log is unreadable; live delivery skipped");
            return Ok(());
        };
        let batch = Batch {
            run_id,
            epoch: &run.epoch,
            first: from,
            sub: None,
            reset: false,
        };
        let Ok(frames) = wire::events_frames(batch, &events) else {
            tracing::error!(run_id, "a session.events head could not be encoded");
            return Ok(());
        };
        self.spend_live(frames.len());
        for frame in frames {
            self.ping_if_due().await?;
            self.record(
                "session.events",
                Some(run_id),
                Some(&run.epoch),
                Some((frame.first, frame.last)),
            );
            self.sink.send(frame.text).await?;
        }
        Ok(())
    }

    async fn answer(&mut self, run_id: &str, sub: &str, after: Option<Cursor>) -> Result<(), ()> {
        // Live events already in the log go out first, so the answer never interleaves with them;
        // the run is read after that, so the answer reaches at least as far as they did.
        self.due.retain(|due| due != run_id);
        self.flush(run_id).await?;
        let run = match self.store.run(run_id) {
            Ok(Some(run)) => run,
            Ok(None) => return self.no_history(run_id, sub).await,
            Err(error) => {
                // Transient: no answer, so the subscriber's own retry asks again.
                tracing::error!(
                    ?error,
                    run_id,
                    "reading a run failed; the subscription is left unanswered"
                );
                return Ok(());
            }
        };
        let end = run.next_seq;
        let reset = after.as_ref().is_none_or(|cursor| {
            cursor.epoch != run.epoch
                || cursor.seq < -1
                || u64::try_from(cursor.seq + 1).is_ok_and(|next| next > end)
        });
        let from = if reset {
            0
        } else {
            after
                .as_ref()
                .and_then(|cursor| u64::try_from(cursor.seq + 1).ok())
                .unwrap_or(0)
        };
        let events = match self.store.events(run_id, &run.epoch, from, end) {
            Ok(events) => events,
            Err(crate::store::StoreError::Corrupt) => {
                tracing::error!(run_id, "run log is unreadable; answering no_history");
                return self.no_history(run_id, sub).await;
            }
            Err(crate::store::StoreError::Sqlite) => {
                tracing::error!(
                    run_id,
                    "reading the run log failed; the subscription is left unanswered"
                );
                return Ok(());
            }
        };
        let batch = Batch {
            run_id,
            epoch: &run.epoch,
            first: from,
            sub: Some(sub),
            reset,
        };
        let Ok(frames) = wire::events_frames(batch, &events) else {
            tracing::error!(
                run_id,
                "a session.events head could not be encoded; answering no_history"
            );
            return self.no_history(run_id, sub).await;
        };
        for frame in frames {
            let wait = self.answers.wait();
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            self.answers.take();
            self.ping_if_due().await?;
            self.record(
                "session.events",
                Some(run_id),
                Some(&run.epoch),
                Some((frame.first, frame.last)),
            );
            self.sink.send(frame.text).await?;
        }
        Ok(())
    }

    /// Answer a subscription with `session.unavailable{sub}` (no usable history).
    async fn no_history(&mut self, run_id: &str, sub: &str) -> Result<(), ()> {
        if let Ok(frame) = wire::unavailable(run_id, Unavailable::Sub(sub)) {
            self.record("session.unavailable", Some(run_id), None, None);
            self.sink.send(frame).await?;
        }
        Ok(())
    }

    fn spend_live(&mut self, frames: usize) {
        let slot = Duration::from_secs(1) / LIVE_FRAMES_PER_SECOND;
        let count = u32::try_from(frames).unwrap_or(u32::MAX);
        self.live_at = self.live_at.max(Instant::now()) + slot * count;
    }

    fn record(
        &self,
        kind: &str,
        run_id: Option<&str>,
        epoch: Option<&str>,
        range: Option<(u64, i64)>,
    ) {
        *self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = LastFrame {
            kind: kind.to_owned(),
            run_id: run_id.map(str::to_owned),
            epoch: epoch.map(str::to_owned),
            range,
        };
    }
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
