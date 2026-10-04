//! Runtime protocol frames: a one-line JSON head, an optional event body, and byte limits.
//!
//! A frame is one WebSocket text message. Only `session.events` carries a body (a JSON array
//! after the first `\n`). Heartbeats are the bare texts `ping` and `pong`, not frames.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Protocol version carried by `runtime.hello`.
pub const PROTOCOL_VERSION: u8 = 1;
/// The only session contract this runtime speaks.
pub const SESSION_CONTRACT: &str = "bonsai.session/1";
/// Largest `session.events` frame, head plus body.
pub const FRAME_BYTES: usize = 256 * 1024;
/// Largest `runtime.hello` head.
pub const HELLO_HEAD_BYTES: usize = 128 * 1024;
/// Largest `run.status` head.
pub const STATUS_HEAD_BYTES: usize = 16 * 1024;
/// Largest head of any other frame.
pub const HEAD_BYTES: usize = 8 * 1024;
/// Largest serialized session event.
pub const EVENT_BYTES: usize = 64 * 1024;
/// Largest `tool.input`, `tool.output` or `ask.detail`.
pub const FIELD_BYTES: usize = 16 * 1024;
/// Largest `final_text`.
pub const FINAL_TEXT_BYTES: usize = 4 * 1024;
/// Largest `reason_detail`.
pub const REASON_DETAIL_BYTES: usize = 1024;
/// Body budget per `session.events` frame; the head stays within [`HEAD_BYTES`].
pub const BODY_BYTES: usize = 200_000;
/// Heartbeat sent by the runtime.
pub const PING: &str = "ping";
/// Heartbeat answer from the Hub.
pub const PONG: &str = "pong";

/// Return the longest prefix of `text` within `max` UTF-8 bytes, ending on a code point.
///
/// # Arguments
///
/// * `text` - Original text.
/// * `max` - Byte budget.
///
/// # Returns
///
/// The prefix and whether anything was cut.
#[must_use]
pub fn truncate(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    (&text[..text.floor_char_boundary(max)], true)
}

/// Remove control characters and cut a display string to `max` bytes.
#[must_use]
pub fn display(text: &str, max: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let (prefix, _) = truncate(clean.trim(), max);
    prefix.to_owned()
}

/// Return whether `value` matches `[A-Za-z0-9._:-]{1,max}`.
#[must_use]
pub fn is_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

/// Return whether `value` is a valid model ID: 1-128 printable ASCII bytes without spaces.
#[must_use]
pub fn is_model_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// A person named by the Hub (`by`, `owner`, `requested_by`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    /// Stable identity such as `github:1`.
    pub id: String,
    /// Display login.
    #[serde(default)]
    pub login: Option<String>,
}

/// Replay position in a run's event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Opaque log generation.
    pub epoch: String,
    /// Last event already held by the subscriber; `-1` means none.
    pub seq: i64,
}

/// Board content selected by the Hub at send time; all fields are untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Note path inside the space.
    pub path: String,
    /// One-based line number of the task.
    pub line: u64,
    /// Raw task line.
    pub text: String,
    /// Section heading, if any.
    #[serde(default)]
    pub heading: Option<String>,
    /// Section text around the task.
    #[serde(default)]
    pub context: String,
}

/// Project chosen by the dispatcher; the ID is the one this runtime announced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    /// Announced project ID.
    pub id: String,
}

/// Bonsai write endpoint for this run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BonsaiRef {
    /// The only MCP URL agents may use to change Bonsai.
    pub mcp_url: String,
}

/// Who dispatched the run; `owner` is informational, never a gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requester {
    /// Stable identity such as `github:2`.
    pub id: String,
    /// Display login.
    #[serde(default)]
    pub login: Option<String>,
    /// Whether the dispatcher paired this runtime.
    #[serde(default)]
    pub owner: bool,
}

/// `run.dispatch`: a request to start one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dispatch {
    /// Idempotency key and run identity.
    pub run_id: String,
    /// Space the task belongs to.
    pub space_id: String,
    /// Task content.
    pub task: Task,
    /// Target project.
    pub project: ProjectRef,
    /// Provider, or `None` for the announced default.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model, or `None` for the provider default.
    #[serde(default)]
    pub model: Option<String>,
    /// Dispatcher's note; empty when absent.
    #[serde(default)]
    pub instruction: String,
    /// Filled harness settings for the resolved provider.
    #[serde(default)]
    pub settings: Map<String, Value>,
    /// Server-written closing instruction in the space's language.
    #[serde(default)]
    pub wrapup: String,
    /// Bonsai write endpoint.
    pub bonsai: BonsaiRef,
    /// Dispatcher.
    pub requested_by: Requester,
    /// Session contract requested by the Hub.
    #[serde(default)]
    pub session: String,
}

/// `runtime.welcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// Machine owner; local input is attributed to this person.
    pub owner: Person,
}

/// `session.answer` relayed by the Hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// Run the request belongs to.
    pub run_id: String,
    /// Member who answered.
    pub by: Person,
    /// Request ID from the `ask` event.
    pub ask_id: String,
    /// Chosen option ID.
    pub option_id: String,
    /// Question answers keyed by `questions[].key`.
    #[serde(default)]
    pub answers: Option<Map<String, Value>>,
    /// Free text for the agent, mainly on deny.
    #[serde(default)]
    pub note: Option<String>,
}

/// Frames the Hub sends to a runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Inbound {
    /// Handshake accepted.
    #[serde(rename = "runtime.welcome")]
    Welcome(Welcome),
    /// Start a run.
    #[serde(rename = "run.dispatch")]
    Dispatch(Box<Dispatch>),
    /// Report a run's current status.
    #[serde(rename = "run.query")]
    Query {
        /// Run in question.
        run_id: String,
    },
    /// Cancel a run.
    #[serde(rename = "run.cancel")]
    Cancel {
        /// Run to cancel.
        run_id: String,
    },
    /// Replay a run's log after a cursor.
    #[serde(rename = "session.subscribe")]
    Subscribe {
        /// Run to replay.
        run_id: String,
        /// Subscription ID echoed on every answering frame.
        sub: String,
        /// Cursor, or `None` to replay from the start.
        #[serde(default)]
        after: Option<Cursor>,
    },
    /// Member input for the session.
    #[serde(rename = "session.send")]
    Send {
        /// Target run.
        run_id: String,
        /// Sender.
        by: Person,
        /// Idempotency key: 32 lowercase hex digits.
        input_id: String,
        /// Message text.
        text: String,
    },
    /// Interrupt the current turn.
    #[serde(rename = "session.interrupt")]
    Interrupt {
        /// Target run.
        run_id: String,
    },
    /// Answer an `ask`.
    #[serde(rename = "session.answer")]
    Answer(Answer),
    /// Any frame type this runtime does not know; ignored.
    #[serde(other)]
    Unknown,
}

/// One decoded WebSocket text message from the Hub.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Heartbeat answer.
    Pong,
    /// A frame.
    Frame(Inbound),
    /// A known frame type whose fields could not be read; carries the run when present.
    Malformed {
        /// Frame type.
        kind: String,
        /// Run named in the head, if readable.
        run_id: Option<String>,
    },
}

/// Decode one text message from the Hub.
///
/// # Arguments
///
/// * `text` - Raw WebSocket text.
///
/// # Returns
///
/// [`Incoming::Frame`] for readable frames (unknown types become [`Inbound::Unknown`]),
/// [`Incoming::Malformed`] when the head is JSON but a known frame's fields are wrong.
///
/// # Errors
///
/// Returns the JSON error when the head is not a JSON object with a string `type`.
pub fn decode(text: &str) -> Result<Incoming, serde_json::Error> {
    if text == PONG {
        return Ok(Incoming::Pong);
    }
    let head = text.split_once('\n').map_or(text, |(head, _)| head);
    let value: Value = serde_json::from_str(head)?;
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| serde::de::Error::custom("frame head without a string type"))?;
    let run_id = value
        .get("run_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(serde_json::from_value::<Inbound>(value)
        .map_or(Incoming::Malformed { kind, run_id }, Incoming::Frame))
}

/// Run states a runtime reports; `queued` belongs to Bonsai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Accepted; will never be started twice.
    Claimed,
    /// Session started.
    Running,
    /// Session ended normally.
    Completed,
    /// Infrastructure or provider failure.
    Failed,
    /// Stopped on request, confirmed.
    Cancelled,
}

impl RunState {
    /// Return whether the state is final.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Stable storage spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse the storage spelling.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claimed" => Some(Self::Claimed),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Failure reasons a runtime may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// Unknown project ID or its directory is gone.
    ProjectUnavailable,
    /// Provider not available on this machine.
    ProviderUnavailable,
    /// Provider failed.
    ProviderError,
    /// Session could not be recovered after a restart.
    SessionLost,
    /// No record of the run.
    RunUnknown,
    /// Local policy or validation rejected the run.
    Rejected,
}

impl ReasonCode {
    /// Stable storage spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectUnavailable => "project_unavailable",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderError => "provider_error",
            Self::SessionLost => "session_lost",
            Self::RunUnknown => "run_unknown",
            Self::Rejected => "rejected",
        }
    }

    /// Parse the storage spelling.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "project_unavailable" => Some(Self::ProjectUnavailable),
            "provider_unavailable" => Some(Self::ProviderUnavailable),
            "provider_error" => Some(Self::ProviderError),
            "session_lost" => Some(Self::SessionLost),
            "run_unknown" => Some(Self::RunUnknown),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// Resolved execution reported on every status from `claimed` on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    /// Resolved provider.
    pub provider: String,
    /// Resolved model, if known.
    pub model: Option<String>,
    /// Whether tool calls stop for human approval.
    pub approvals: bool,
    /// Whether agents may write this Bonsai through the dispatch MCP URL.
    pub bonsai_write: bool,
}

/// `run.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Always `run.status`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Run reported on.
    pub run_id: String,
    /// Current state.
    pub status: RunState,
    /// Local Unix time in milliseconds when the state was reached.
    pub at: i64,
    /// Resolved execution; required from `claimed` on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
    /// Failure reason, only on `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<ReasonCode>,
    /// Failure detail, at most [`REASON_DETAIL_BYTES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_detail: Option<String>,
    /// Start of the last assistant message, at most [`FINAL_TEXT_BYTES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_text: Option<String>,
}

/// Where a `session.unavailable` is addressed: one subscription or one input/answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable<'a> {
    /// Answer to the subscription with this ID.
    Sub(&'a str),
    /// About the input or answer with this ID.
    Ref(&'a str),
}

/// Frame-size violations detected before sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// The head exceeds its limit.
    #[error("frame head exceeds {limit} bytes")]
    Head {
        /// Applicable limit.
        limit: usize,
    },
    /// The head could not be serialized.
    #[error("frame head cannot be serialized")]
    Json,
}

/// Whether `value` has the shape of a Bonsai run ID: `r_` plus 32 lowercase hex digits.
#[must_use]
pub fn is_run_id(value: &str) -> bool {
    value.strip_prefix("r_").is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Encode a `run.status` head, halving `final_text` and `reason_detail` until it fits.
///
/// Both are limited in raw bytes, but escaping can grow control characters sixfold; a status
/// that never fits would never reach the Hub, and the run would never end there.
///
/// # Errors
///
/// Returns [`EncodeError::Head`] when it exceeds [`STATUS_HEAD_BYTES`] with both texts empty.
pub fn status(status: &Status) -> Result<String, EncodeError> {
    let mut status = status.clone();
    loop {
        match head(&status, STATUS_HEAD_BYTES) {
            Err(EncodeError::Head { .. }) => {}
            done => return done,
        }
        let longest = [status.final_text.as_mut(), status.reason_detail.as_mut()]
            .into_iter()
            .flatten()
            .max_by_key(|text| text.len())
            .filter(|text| !text.is_empty());
        let Some(text) = longest else {
            return Err(EncodeError::Head {
                limit: STATUS_HEAD_BYTES,
            });
        };
        let keep = text.floor_char_boundary(text.len() / 2);
        text.truncate(keep);
    }
}

/// Encode a `runtime.hello` head.
///
/// # Errors
///
/// Returns [`EncodeError::Head`] above [`HELLO_HEAD_BYTES`].
pub fn hello(hello: &impl Serialize) -> Result<String, EncodeError> {
    head(hello, HELLO_HEAD_BYTES)
}

/// Encode a `session.unavailable{reason:"no_history"}` addressed to a sub or a ref.
///
/// # Errors
///
/// Returns [`EncodeError::Head`] above [`HEAD_BYTES`].
pub fn unavailable(run_id: &str, target: Unavailable<'_>) -> Result<String, EncodeError> {
    let mut head_value = serde_json::json!({
        "type": "session.unavailable",
        "run_id": run_id,
        "reason": "no_history",
    });
    match target {
        Unavailable::Sub(sub) => head_value["sub"] = Value::from(sub),
        Unavailable::Ref(reference) => head_value["ref"] = Value::from(reference),
    }
    head(&head_value, HEAD_BYTES)
}

/// Placement of a run of events inside `session.events` frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Batch<'a> {
    /// Run the events belong to.
    pub run_id: &'a str,
    /// Log generation.
    pub epoch: &'a str,
    /// Sequence number of the first event.
    pub first: u64,
    /// Subscription answered, or `None` for a live frame.
    pub sub: Option<&'a str>,
    /// Mark the first frame `reset` (replay from zero).
    pub reset: bool,
}

/// One encoded `session.events` frame and the sequence numbers it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventsFrame {
    /// Head line, newline and JSON array body.
    pub text: String,
    /// First sequence number.
    pub first: u64,
    /// Last sequence number (`first - 1` for an empty frame).
    pub last: i64,
}

/// Split consecutive serialized events into `session.events` frames.
///
/// Every frame's events are consecutive; a body stays within [`BODY_BYTES`]. With no events a
/// single empty frame (`last = first - 1`) is produced. Answering frames carry `sub`; the last
/// one carries `sync` and, on reset, the first one carries `reset`. An event over
/// [`EVENT_BYTES`] (stored before events were fitted) is sent as an error notice with the same
/// `seq`, so one bad event never stops a whole answer or live batch.
///
/// # Arguments
///
/// * `batch` - Run, epoch, first sequence number and addressing.
/// * `events` - Serialized events, already numbered from `batch.first`.
///
/// # Errors
///
/// Returns [`EncodeError`] only when a head cannot be encoded.
pub fn events_frames(batch: Batch<'_>, events: &[String]) -> Result<Vec<EventsFrame>, EncodeError> {
    let fitted: Vec<String>;
    let events = if events.iter().any(|event| event.len() > EVENT_BYTES) {
        fitted = events
            .iter()
            .zip(batch.first..)
            .map(|(event, seq)| {
                if event.len() <= EVENT_BYTES {
                    return event.clone();
                }
                tracing::error!(seq, "a stored session event exceeds the event limit");
                crate::event::Event {
                    seq,
                    at: serde_json::from_str::<Value>(event)
                        .ok()
                        .and_then(|value| value["at"].as_i64())
                        .unwrap_or_default(),
                    body: crate::event::Body::Notice {
                        level: crate::event::Level::Error,
                        text: crate::event::OVERSIZE_NOTICE.to_owned(),
                    },
                }
                .to_json()
                .unwrap_or_default()
            })
            .collect();
        &fitted[..]
    } else {
        events
    };
    let mut chunks: Vec<&[String]> = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (index, event) in events.iter().enumerate() {
        let size = event.len() + 1;
        if index > start && bytes + size > BODY_BYTES {
            chunks.push(&events[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += size;
    }
    if start < events.len() || chunks.is_empty() {
        chunks.push(&events[start..]);
    }
    let last_index = chunks.len() - 1;
    let mut first = batch.first;
    let mut frames = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.into_iter().enumerate() {
        let next = first + u64::try_from(chunk.len()).map_err(|_| EncodeError::Json)?;
        let last = i64::try_from(next).map_err(|_| EncodeError::Json)? - 1;
        let mut head_value = serde_json::json!({
            "type": "session.events",
            "run_id": batch.run_id,
            "epoch": batch.epoch,
            "first": first,
            "last": last,
        });
        if let Some(sub) = batch.sub {
            head_value["sub"] = Value::from(sub);
            if batch.reset && index == 0 {
                head_value["reset"] = Value::Bool(true);
            }
            if index == last_index {
                head_value["sync"] = Value::Bool(true);
            }
        }
        let mut frame = head(&head_value, HEAD_BYTES)?;
        frame.push('\n');
        frame.push('[');
        frame.push_str(&chunk.join(","));
        frame.push(']');
        frames.push(EventsFrame {
            text: frame,
            first,
            last,
        });
        first = next;
    }
    Ok(frames)
}

/// Encode an empty live frame announcing a new epoch (`first: 0, last: -1`).
///
/// # Errors
///
/// Returns [`EncodeError::Head`] above [`HEAD_BYTES`].
pub fn new_epoch(run_id: &str, epoch: &str) -> Result<String, EncodeError> {
    let batch = Batch {
        run_id,
        epoch,
        first: 0,
        sub: None,
        reset: false,
    };
    events_frames(batch, &[])?
        .pop()
        .map(|frame| frame.text)
        .ok_or(EncodeError::Json)
}

fn head(value: &impl Serialize, limit: usize) -> Result<String, EncodeError> {
    let text = serde_json::to_string(value).map_err(|_| EncodeError::Json)?;
    if text.len() > limit {
        return Err(EncodeError::Head { limit });
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
