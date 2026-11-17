//! Bookkeeping shared by the single-topic BTW side-question runners.
//!
//! Each adapter owns how its auxiliary process is spawned, how native output is parsed and how a
//! running turn is interrupted. This module owns what those runners otherwise repeat around that:
//! ask validation and topic construction, the frozen prompt envelope and its context limit, the
//! panel projection and change tracking, and settling the auxiliary cleanup task.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tokio::task::JoinHandle;

use provider_core::ports::agent_session::AgentSessionError;
use provider_core::ports::side_questions::{SideAsk, SideRunnerSpec};
use provider_core::protocol::side_questions::{
    BtwHistoryRecord, BtwHistoryTurn, MAX_QUESTION_BYTES, SideQuestionsState, SideStrategy,
    SideTopic, SideTurnStatus, reason,
};

/// Largest rendered prompt (instructions, frozen context and prior answers) a runner submits.
const MAX_PROMPT_BYTES: usize = 192 * 1024;
/// Panel error published when a topic outgrows [`MAX_PROMPT_BYTES`].
const CONTEXT_LIMIT_MESSAGE: &str =
    "BTW context limit reached. Expand this side chat to a full session to continue.";

/// Milliseconds since the Unix epoch; a clock before the epoch reads as zero and overflow
/// saturates instead of panicking.
#[must_use]
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Serialize the frozen snapshot, the prior topic record and the question as one JSON object.
///
/// Runners prepend their own instructions when the native interface has no separate channel
/// for them.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] when the ask cannot be serialized.
pub fn envelope(ask: &SideAsk) -> Result<String, AgentSessionError> {
    serde_json::to_string(&json!({
        "snapshot": ask.snapshot,
        "previous": ask.previous.as_ref().map(|topic| &topic.record),
        "question": ask.question,
    }))
    .map_err(|_| AgentSessionError::Rejected)
}

/// Validate an ask against the runner's session and build the running topic for it.
///
/// A follow-up appends a running turn to the supplied previous topic, which must be the named
/// record, use `strategy` and have a complete latest turn. A new ask starts a topic anchored at
/// the snapshot's native leaf.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] for an empty request id, an empty or oversized
/// question, a snapshot of another provider, model or version, or an inconsistent follow-up.
pub fn topic(
    ask: &SideAsk,
    spec: &SideRunnerSpec,
    strategy: SideStrategy,
) -> Result<SideTopic, AgentSessionError> {
    if ask.request_id.is_empty()
        || ask.question.trim().is_empty()
        || ask.question.len() > MAX_QUESTION_BYTES
        || ask.snapshot.provider != spec.session.provider
        || ask.snapshot.model != spec.session.config.model
        || ask.snapshot.v != 1
    {
        return Err(AgentSessionError::Rejected);
    }
    let now = now_millis();
    if let Some(id) = &ask.record_id {
        let mut previous = ask.previous.clone().ok_or(AgentSessionError::Rejected)?;
        if previous.record.id != *id
            || previous.strategy != strategy
            || previous.latest_turn().status != SideTurnStatus::Complete
        {
            return Err(AgentSessionError::Rejected);
        }
        previous.record.follow_ups.push(BtwHistoryTurn {
            question: ask.question.clone(),
            images: Vec::new(),
            answer: String::new(),
            status: SideTurnStatus::Running,
            created_at: now,
            updated_at: now,
            error: None,
        });
        return Ok(previous);
    }
    if ask.previous.is_some() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(SideTopic {
        record: BtwHistoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            question: ask.question.clone(),
            images: Vec::new(),
            answer: String::new(),
            status: SideTurnStatus::Running,
            created_at: now,
            updated_at: now,
            error: None,
            leaf_id: ask.snapshot.native_leaf_id.clone(),
            title: None,
            follow_ups: Vec::new(),
        },
        strategy,
        truncated: ask.snapshot.truncated,
    })
}

/// Mutable view of a topic's latest turn (its last follow-up, or the root record) together with
/// the topic-wide truncation flag.
#[derive(Debug)]
pub struct LatestTurn<'a> {
    /// Answer text of the latest turn.
    pub answer: &'a mut String,
    /// Lifecycle of the latest turn.
    pub status: &'a mut SideTurnStatus,
    /// Last update time of the latest turn, in milliseconds since the Unix epoch.
    pub updated_at: &'a mut u64,
    /// Redacted failure text of the latest turn.
    pub error: &'a mut Option<String>,
    /// Whether any part of the topic was clipped.
    pub truncated: &'a mut bool,
}

/// Borrow the latest turn of `topic` for in-place updates.
#[must_use]
pub fn latest_turn_mut(topic: &mut SideTopic) -> LatestTurn<'_> {
    let truncated = &mut topic.truncated;
    match topic.record.follow_ups.last_mut() {
        Some(turn) => LatestTurn {
            answer: &mut turn.answer,
            status: &mut turn.status,
            updated_at: &mut turn.updated_at,
            error: &mut turn.error,
            truncated,
        },
        None => LatestTurn {
            answer: &mut topic.record.answer,
            status: &mut topic.record.status,
            updated_at: &mut topic.record.updated_at,
            error: &mut topic.record.error,
            truncated,
        },
    }
}

/// Panel state of a runner that keeps one topic and executes at most one turn at a time.
///
/// The runtime remains the durable topic store; the panel holds the latest topic, the request
/// that produced it, the panel-scoped context-limit error and whether the projection changed.
#[derive(Debug, Default)]
pub struct Panel {
    topic: Option<SideTopic>,
    request_id: Option<String>,
    error: Option<String>,
    changed: bool,
}

impl Panel {
    /// Start an ask: clear the previous context-limit error, then refuse a concurrent or
    /// repeated request.
    ///
    /// `running` reports whether the runner still owns a turn or its cleanup.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Busy`] while a turn runs and [`AgentSessionError::Failed`]
    /// when `request_id` repeats the latest request, which must never execute again.
    pub fn begin(&mut self, request_id: &str, running: bool) -> Result<(), AgentSessionError> {
        self.changed |= self.error.take().is_some();
        if running {
            return Err(AgentSessionError::Busy);
        }
        if self.request_id.as_deref() == Some(request_id) {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    /// Refuse a rendered prompt over the context limit and publish the panel error for it.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Rejected`] when `prompt` exceeds the limit; the rejection
    /// reason then reads `context_limit` until the next ask.
    pub fn admit(&mut self, prompt: &str) -> Result<(), AgentSessionError> {
        if prompt.len() > MAX_PROMPT_BYTES {
            self.error = Some(CONTEXT_LIMIT_MESSAGE.to_owned());
            self.changed = true;
            return Err(AgentSessionError::Rejected);
        }
        Ok(())
    }

    /// Record the topic and request of a turn the runner now owns.
    pub fn install(&mut self, topic: SideTopic, request_id: String) {
        self.topic = Some(topic);
        self.request_id = Some(request_id);
    }

    /// The latest topic, if any ask was installed.
    #[must_use]
    pub fn topic(&self) -> Option<&SideTopic> {
        self.topic.as_ref()
    }

    /// Mutable access to the latest topic; callers report changes with [`Self::mark_changed`].
    pub fn topic_mut(&mut self) -> Option<&mut SideTopic> {
        self.topic.as_mut()
    }

    /// Request id of the latest installed ask.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Note that the projection changed since the last poll.
    pub fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Return and reset whether the projection changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// `context_limit` while the latest ask was refused for its prompt size.
    #[must_use]
    pub fn rejection_reason(&self) -> Option<&'static str> {
        self.error.as_ref().map(|_| reason::CONTEXT_LIMIT)
    }

    /// Whether a cancel addressed to `record_id` targets this panel's topic; `None` targets any.
    #[must_use]
    pub fn owns(&self, record_id: Option<&str>) -> bool {
        record_id.is_none_or(|id| {
            self.topic
                .as_ref()
                .is_some_and(|topic| topic.record.id == id)
        })
    }

    /// Lifecycle of the latest turn of the topic, if any.
    #[must_use]
    pub fn latest_status(&self) -> Option<SideTurnStatus> {
        self.topic.as_ref().map(|topic| topic.latest_turn().status)
    }

    /// Retained topics, at most one.
    #[must_use]
    pub fn history(&self) -> Vec<SideTopic> {
        self.topic.iter().cloned().collect()
    }

    /// Project the panel; `running` marks the topic as the running one.
    #[must_use]
    pub fn projection(&self, running: bool) -> SideQuestionsState {
        let id = self.topic.as_ref().map(|topic| topic.record.id.clone());
        SideQuestionsState {
            error: self.error.clone(),
            records: self.topic.iter().cloned().collect(),
            reveal: id.clone(),
            running_id: if running { id } else { None },
            ..SideQuestionsState::default()
        }
    }

    /// Record a redacted native failure on the latest turn, reported if the turn errors.
    pub fn native_error(&mut self, error: &str) {
        if let Some(topic) = &mut self.topic {
            *latest_turn_mut(topic).error = crate::diagnostics::failure::redact_text(error);
        }
    }

    /// Settle the latest turn with `status`.
    ///
    /// An error keeps the native failure recorded by [`Self::native_error`] and otherwise
    /// reports `fallback`; any other status clears the failure text.
    pub fn finish(&mut self, status: SideTurnStatus, fallback: &str) {
        if let Some(topic) = &mut self.topic {
            let turn = latest_turn_mut(topic);
            let error = (status == SideTurnStatus::Error)
                .then(|| turn.error.take().unwrap_or_else(|| fallback.to_owned()));
            *turn.status = status;
            *turn.updated_at = now_millis();
            *turn.error = error;
            self.changed = true;
        }
    }
}

/// Background shutdown of a settled auxiliary process; its output is the turn's final status.
///
/// The latest turn becomes terminal only once the cleanup finished, so a terminal status always
/// follows the release of the auxiliary process.
#[derive(Debug, Default)]
pub struct Cleanup {
    task: Option<JoinHandle<Result<SideTurnStatus, AgentSessionError>>>,
}

impl Cleanup {
    /// Spawn the shutdown of a settled auxiliary process.
    pub fn start(
        &mut self,
        task: impl Future<Output = Result<SideTurnStatus, AgentSessionError>> + Send + 'static,
    ) {
        self.task = Some(tokio::spawn(task));
    }

    /// Whether a shutdown still has to be settled.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.task.is_some()
    }

    /// Settle a finished shutdown into `panel` without waiting; a failed shutdown is an error.
    pub fn reap(&mut self, panel: &mut Panel, fallback: &str) {
        let Some(task) = self.task.as_mut().filter(|task| task.is_finished()) else {
            return;
        };
        if let Poll::Ready(result) = Pin::new(task).poll(&mut Context::from_waker(Waker::noop())) {
            self.task = None;
            panel.finish(
                match result {
                    Ok(Ok(status)) => status,
                    _ => SideTurnStatus::Error,
                },
                fallback,
            );
        }
    }

    /// Wait for the pending shutdown, if any, and settle it into `panel`.
    ///
    /// The task stays owned by `self` while awaited, so cancelling this future leaves its
    /// result to a later call.
    ///
    /// # Errors
    ///
    /// Returns the shutdown failure, or [`AgentSessionError::Failed`] when the task panicked;
    /// the latest turn is then settled as an error.
    pub async fn settle(
        &mut self,
        panel: &mut Panel,
        fallback: &str,
    ) -> Result<(), AgentSessionError> {
        let Some(task) = &mut self.task else {
            return Ok(());
        };
        let result = task
            .await
            .map_err(|_| AgentSessionError::Failed)
            .and_then(|result| result);
        self.task = None;
        panel.finish(
            result.as_ref().copied().unwrap_or(SideTurnStatus::Error),
            fallback,
        );
        result?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
