//! OMP token, cost and context usage (D26, M11), read as Paseo's `OmpUsagePoller` reads it:
//! `get_session_stats` every 3 s while a turn runs and once more when it ends or after
//! `/compact`, publishing only changed snapshots. Each read is a background request, so it
//! never blocks event polling or the caller. An OMP that refuses `get_session_stats` is read
//! through `get_state.contextUsage` instead (COMPAT, Paseo's `ompGetStateFallback`).

use std::time::Duration;

use serde_json::Value;
use tokio::{sync::oneshot, time::Instant};

use super::{LABEL, transport::Transport};
use provider_core::ports::agent_session::AgentSessionError;
use provider_core::protocol::usage::AgentUsage;

/// Interval between reads while a turn runs, as Paseo polls.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Wait for a turn's final read before its outcome is published without it.
const OUTCOME_GRACE: Duration = Duration::from_secs(2);

/// The native command a read sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `get_session_stats`: tokens, cost and context usage of the session.
    Stats,
    /// `get_state`: context usage only, for an OMP without `get_session_stats`.
    State,
}

impl Source {
    const fn command(self) -> &'static str {
        match self {
            Self::Stats => "get_session_stats",
            Self::State => "get_state",
        }
    }
}

/// One background usage request awaiting OMP's response.
#[derive(Debug)]
struct Read {
    id: String,
    source: Source,
    written: Option<oneshot::Receiver<Result<(), AgentSessionError>>>,
    /// The response is abandoned after the request timeout.
    deadline: Instant,
}

/// Usage reads of one session and the last snapshot it published.
#[derive(Debug, Default)]
pub(super) struct Poller {
    /// OMP refused `get_session_stats`, so reads use `get_state`.
    stats_refused: bool,
    read: Option<Read>,
    /// A read is due now rather than at the next interval.
    wanted: bool,
    /// The next periodic read while a turn runs.
    next: Option<Instant>,
    /// The ended turn's outcome waits for its final read until this instant.
    outcome: Option<Instant>,
    last: Option<AgentUsage>,
}

impl Poller {
    /// Read periodically while the turn that just started runs.
    pub(super) fn start_turn(&mut self) {
        self.next = Some(Instant::now() + POLL_INTERVAL);
    }

    /// Stop periodic reads and read once more; the turn's outcome waits briefly for it.
    pub(super) fn complete_turn(&mut self) {
        self.next = None;
        self.wanted = true;
        self.outcome = Some(Instant::now() + OUTCOME_GRACE);
    }

    /// Read once more, such as after `/compact` changed the context.
    pub(super) fn refresh(&mut self) {
        self.wanted = true;
    }

    /// Stop reading, as the connection failed or closed.
    pub(super) fn stop(&mut self) {
        self.next = None;
        self.wanted = false;
        self.outcome = None;
    }

    /// Forget the request in flight: a new process cannot answer the old one's ids.
    pub(super) fn reset(&mut self) {
        self.read = None;
    }

    /// Whether a just-ended turn's outcome still waits for its final read.
    pub(super) fn holds_outcome(&self) -> bool {
        self.outcome
            .is_some_and(|deadline| Instant::now() < deadline)
    }

    /// Whether `event` is the response to the read in flight.
    pub(super) fn owns(&self, event: &Value) -> bool {
        self.read
            .as_ref()
            .is_some_and(|read| event["id"] == read.id.as_str())
    }

    /// Take the response to the read in flight and return its snapshot if it changed.
    /// `totals` are the token and cost sums of the messages seen, which only a `get_state`
    /// read reports with OMP's context usage. A refused `get_session_stats` switches every
    /// later read to `get_state` and reads again at once.
    pub(super) fn answer(&mut self, event: &Value, totals: &AgentUsage) -> Option<AgentUsage> {
        let read = self.read.take()?;
        let data = &event["data"];
        let usage = match (read.source, event["success"] == true) {
            (Source::Stats, true) if reports_stats(data) => stats(data),
            (Source::Stats, _) => {
                tracing::debug!(
                    provider = LABEL,
                    "OMP has no session stats; reading its context usage instead"
                );
                self.stats_refused = true;
                self.wanted = true;
                return None;
            }
            (Source::State, true) => state(data, totals),
            (Source::State, false) => None,
        };
        self.outcome = None;
        let usage = usage.filter(|usage| {
            let valid = usage.is_valid();
            if !valid {
                tracing::debug!(provider = LABEL, "Ignoring invalid OMP usage");
            }
            valid
        })?;
        if self.last.as_ref() == Some(&usage) {
            return None;
        }
        self.last = Some(usage.clone());
        Some(usage)
    }

    /// Send a due read in the background, one at a time; requests abandoned after `limit`
    /// or whose write failed are dropped. A busy writer defers the read to the next poll.
    pub(super) fn poll(&mut self, transport: &mut Transport, limit: Duration) {
        if let Some(read) = &mut self.read {
            let failed = match read.written.as_mut().map(oneshot::Receiver::try_recv) {
                Some(Ok(Ok(()))) => {
                    read.written = None;
                    false
                }
                None | Some(Err(oneshot::error::TryRecvError::Empty)) => false,
                Some(Ok(Err(_)) | Err(oneshot::error::TryRecvError::Closed)) => true,
            };
            if !failed && Instant::now() < read.deadline {
                return;
            }
            tracing::debug!(provider = LABEL, "OMP did not answer a usage read");
            self.read = None;
            self.outcome = None;
        }
        let now = Instant::now();
        if !self.wanted && self.next.is_none_or(|next| now < next) {
            return;
        }
        let source = if self.stats_refused {
            Source::State
        } else {
            Source::Stats
        };
        match transport.background(source.command()) {
            Ok((id, written)) => {
                self.read = Some(Read {
                    id,
                    source,
                    written: Some(written),
                    deadline: now + limit,
                });
                self.wanted = false;
                if self.next.is_some() {
                    self.next = Some(now + POLL_INTERVAL);
                }
            }
            Err(AgentSessionError::Busy) => {}
            Err(_) => self.stop(),
        }
    }
}

/// Whether a `get_session_stats` response reports anything, as Paseo checks before falling
/// back to `get_state`.
fn reports_stats(data: &Value) -> bool {
    ["tokens", "cost", "contextUsage"]
        .iter()
        .any(|field| !data[field].is_null())
}

/// Paseo's `toAgentUsage`: input, cache-read and output tokens and cost (zero when absent),
/// with OMP's context window and the tokens in it; none when it reports nothing.
fn stats(data: &Value) -> Option<AgentUsage> {
    let tokens = |field: &str| data["tokens"][field].as_u64().unwrap_or_default();
    let usage = AgentUsage {
        input_tokens: Some(tokens("input")),
        cached_input_tokens: Some(tokens("cacheRead")),
        output_tokens: Some(tokens("output")),
        total_cost_usd: Some(data["cost"].as_f64().unwrap_or_default()),
        context_window_max_tokens: data["contextUsage"]["contextWindow"].as_u64(),
        context_window_used_tokens: data["contextUsage"]["tokens"].as_u64(),
    };
    let empty = AgentUsage {
        input_tokens: Some(0),
        cached_input_tokens: Some(0),
        output_tokens: Some(0),
        total_cost_usd: Some(0.0),
        ..AgentUsage::default()
    };
    (usage != empty).then_some(usage)
}

/// Usage from `get_state`: its `contextUsage` (the window falling back to the model's), with
/// the message `totals` for tokens and cost, which Paseo reports as zero in this fallback.
fn state(state: &Value, totals: &AgentUsage) -> Option<AgentUsage> {
    let context = &state["contextUsage"];
    let usage = AgentUsage {
        context_window_max_tokens: context["contextWindow"]
            .as_u64()
            .or_else(|| state["model"]["contextWindow"].as_u64()),
        context_window_used_tokens: context["tokens"]
            .as_u64()
            .or(totals.context_window_used_tokens),
        ..totals.clone()
    };
    (usage != AgentUsage::default()).then_some(usage)
}

#[cfg(test)]
mod tests;
