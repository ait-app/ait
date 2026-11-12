//! Live per-request usage for the harness usage ledger (ADR-246).
//!
//! Only live sessions feed these trackers. Native history replay restores the usage snapshot
//! alone and never emits request segments.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::cumulative::UsageCounters;
use provider_core::ports::usage_ledger::{RequestUsage, UsageBoundary};

/// Longest segment identity emitted, matching the Codex turn identity bound.
pub(crate) const MAX_SEGMENT_ID: usize = 512;

/// Longest native response id embedded in a segment identity.
const MAX_RESPONSE_ID: usize = 256;

/// Longest model name copied from a native frame.
const MAX_MODEL: usize = 256;

/// Usage of one native request while its frames are still arriving.
#[derive(Debug, Default)]
struct Request {
    /// Native request identity, unique within the turn.
    id: String,
    /// Model the provider reported for the request.
    model: Option<String>,
    /// Disjoint token buckets, merged field-wise by maximum.
    counters: UsageCounters,
    /// One-hour subset of `counters.cache_write`.
    cache_write_1h: Option<u64>,
    /// Whether the terminal native frame arrived.
    complete: bool,
}

impl Request {
    /// Merge one Anthropic `usage` object by taking the maximum of each reported field.
    fn merge(&mut self, usage: &Value) {
        let field = |name: &str| usage[name].as_u64().unwrap_or_default();
        let counters = &mut self.counters;
        counters.input = counters.input.max(field("input_tokens"));
        counters.output = counters.output.max(field("output_tokens"));
        counters.cache_read = counters.cache_read.max(field("cache_read_input_tokens"));
        counters.cache_write = counters
            .cache_write
            .max(field("cache_creation_input_tokens"));
        if let Some(hour) = usage["cache_creation"]["ephemeral_1h_input_tokens"].as_u64() {
            self.cache_write_1h = Some(self.cache_write_1h.map_or(hour, |old| old.max(hour)));
        }
    }

    /// Merge an Anthropic message's model and usage.
    fn absorb(&mut self, message: &Value) {
        if self.model.is_none() {
            self.model = message["model"]
                .as_str()
                .filter(|model| !model.is_empty() && model.len() <= MAX_MODEL)
                .map(str::to_owned);
        }
        self.merge(&message["usage"]);
    }
}

/// Live Claude request usage: one segment per Anthropic message id and stream.
///
/// The foreground stream and each sub-agent stream (keyed by `parent_tool_use_id`) hold at most
/// one open message. A message is emitted when the next message of its stream starts or the
/// turn ends, so its merged value is final.
#[derive(Debug, Default)]
pub(crate) struct ClaudeRequests {
    /// Open message of the foreground stream.
    foreground: Option<Request>,
    /// Open message of each sub-agent stream, keyed by its `parent_tool_use_id`.
    children: BTreeMap<String, Request>,
    /// Message identities already emitted in this turn; later frames for them are dropped.
    emitted: BTreeSet<String>,
    /// Buckets emitted since the last foreground result.
    attributed: UsageCounters,
    /// One-hour cache write subset emitted since the last foreground result.
    attributed_1h: u64,
    /// Whether the foreground stream carried per-message usage since the last result.
    foreground_seen: bool,
    /// Foreground results observed in this turn.
    results: u64,
    /// Whether the turn's terminal event is queued; later frames belong to no turn.
    settled: bool,
}

impl ClaudeRequests {
    /// Forget all state when a new turn starts.
    pub(crate) fn begin(&mut self) {
        *self = Self::default();
    }

    /// Stop observing the turn once its terminal event is queued, until the next `begin`.
    ///
    /// Frames consumed before the terminal event is delivered must not add segments after it.
    pub(crate) fn settle(&mut self) {
        self.settled = true;
    }

    /// Observe one live native frame of turn `turn`.
    ///
    /// Returns the previous message of the frame's stream once a different message starts, and
    /// `None` after the turn settled.
    pub(crate) fn observe(&mut self, record: &Value, turn: &str) -> Option<RequestUsage> {
        if self.settled {
            return None;
        }
        let parent = record["parent_tool_use_id"].as_str();
        // A sidechain frame without its tool call cannot be attributed to a sub-agent stream.
        if parent.is_none() && record["isSidechain"] == true {
            return None;
        }
        let event = &record["event"];
        match (record["type"].as_str(), event["type"].as_str()) {
            (Some("stream_event"), Some("message_start")) => {
                self.open(parent, &event["message"], turn)
            }
            (Some("stream_event"), Some("message_delta")) => {
                let open = match parent {
                    None => self.foreground.as_mut(),
                    Some(parent) => self.children.get_mut(parent),
                }?;
                open.merge(&event["usage"]);
                open.complete = true;
                None
            }
            (Some("assistant"), _) => self.open(parent, &record["message"], turn),
            _ => None,
        }
    }

    /// Emit every open message when a foreground result or cancellation ends the turn.
    ///
    /// With a `result` and no foreground per-message usage since the previous result, also
    /// emits one turn segment of `result.usage` minus what was already attributed. Returns no
    /// segments after the turn settled.
    pub(crate) fn finish(&mut self, turn: &str, result: Option<&Value>) -> Vec<RequestUsage> {
        if self.settled {
            return Vec::new();
        }
        let mut requests = Vec::with_capacity(self.children.len() + 2);
        if let Some(open) = self.foreground.take() {
            requests.extend(self.close(open, None, turn, UsageBoundary::Request));
        }
        for (parent, open) in std::mem::take(&mut self.children) {
            requests.extend(self.close(open, Some(&parent), turn, UsageBoundary::Request));
        }
        if let Some(result) = result.filter(|_| !self.foreground_seen) {
            requests.extend(self.residual(result, turn));
        }
        if result.is_some() {
            self.foreground_seen = false;
            self.attributed = UsageCounters::default();
            self.attributed_1h = 0;
            self.results = self.results.saturating_add(1);
        }
        requests
    }

    /// Merge `message` into its stream, emitting the stream's previous message if it differs.
    fn open(&mut self, parent: Option<&str>, message: &Value, turn: &str) -> Option<RequestUsage> {
        let id = message["id"].as_str().filter(|id| !id.is_empty())?;
        if !message["usage"].is_object() || self.emitted.contains(id) {
            return None;
        }
        self.foreground_seen |= parent.is_none();
        let open = match parent {
            None => self.foreground.as_mut(),
            Some(parent) => self.children.get_mut(parent),
        };
        if let Some(open) = open.filter(|open| open.id == id) {
            open.absorb(message);
            return None;
        }
        let mut next = Request {
            id: id.to_owned(),
            ..Request::default()
        };
        next.absorb(message);
        let previous = match parent {
            None => self.foreground.replace(next),
            Some(parent) => self.children.insert(parent.to_owned(), next),
        }?;
        self.close(previous, parent, turn, UsageBoundary::Request)
    }

    /// Turn segment for the part of `result.usage` that no emitted segment covered.
    fn residual(&mut self, result: &Value, turn: &str) -> Option<RequestUsage> {
        let mut request = Request {
            id: format!("result:{}", self.results),
            complete: true,
            ..Request::default()
        };
        request.merge(result.get("usage").filter(|usage| usage.is_object())?);
        let counters = &mut request.counters;
        counters.input = counters.input.saturating_sub(self.attributed.input);
        counters.output = counters.output.saturating_sub(self.attributed.output);
        counters.cache_read = counters
            .cache_read
            .saturating_sub(self.attributed.cache_read);
        counters.cache_write = counters
            .cache_write
            .saturating_sub(self.attributed.cache_write);
        if *counters == UsageCounters::default() {
            return None;
        }
        request.cache_write_1h = request
            .cache_write_1h
            .map(|hour| hour.saturating_sub(self.attributed_1h));
        self.close(request, None, turn, UsageBoundary::Turn)
    }

    /// Convert a finished message into a ledger record and count it as attributed.
    fn close(
        &mut self,
        request: Request,
        parent: Option<&str>,
        turn: &str,
        boundary: UsageBoundary,
    ) -> Option<RequestUsage> {
        let segment_id = format!("claude:{turn}:{}", request.id);
        self.emitted.insert(request.id);
        let counters = request.counters;
        let cache_write_1h = request
            .cache_write_1h
            .map(|hour| hour.min(counters.cache_write));
        let usage = RequestUsage {
            segment_id,
            parent: parent.map(str::to_owned),
            model: request.model,
            boundary,
            input_tokens: counters.input,
            output_tokens: counters.output,
            cache_read_tokens: counters.cache_read,
            cache_write_tokens: counters.cache_write,
            cache_write_1h_tokens: cache_write_1h,
            reasoning_tokens: None,
            cost_usd_reported: None,
            complete: request.complete,
        };
        if usage.segment_id.len() > MAX_SEGMENT_ID || !usage.is_valid() {
            return None;
        }
        let attributed = &mut self.attributed;
        attributed.input = attributed.input.saturating_add(counters.input);
        attributed.output = attributed.output.saturating_add(counters.output);
        attributed.cache_read = attributed.cache_read.saturating_add(counters.cache_read);
        attributed.cache_write = attributed.cache_write.saturating_add(counters.cache_write);
        self.attributed_1h = self
            .attributed_1h
            .saturating_add(cache_write_1h.unwrap_or_default());
        Some(usage)
    }
}

/// Live Codex request usage of one thread, deduplicated by its running `total.totalTokens`.
#[derive(Debug, Default)]
pub(crate) struct CodexRequests {
    /// Largest running total accepted for this thread.
    accepted_total: Option<u64>,
}

impl CodexRequests {
    /// Observe one `thread/tokenUsage/updated` payload of this thread.
    ///
    /// # Arguments
    ///
    /// * `usage` - The payload's `tokenUsage` object
    /// * `turn` - Active turn the payload is attributed to; `None` for a payload outside it,
    ///   such as the previous turn's usage that a resumed app server reports again
    /// * `child` - Sub-agent thread id when this thread is a child of the session's thread
    ///
    /// # Returns
    ///
    /// A request segment from `last` only when `total.totalTokens` is strictly greater than the
    /// last accepted total and `turn` is known. Duplicate and out-of-order payloads return
    /// `None`; a newer payload without `turn` returns `None` but still advances the cursor.
    pub(crate) fn observe(
        &mut self,
        usage: &Value,
        turn: Option<&str>,
        child: Option<&str>,
    ) -> Option<RequestUsage> {
        let total = field(usage.get("total")?, "totalTokens", "total_tokens")?;
        if self
            .accepted_total
            .is_some_and(|accepted| total <= accepted)
        {
            return None;
        }
        let Some(turn) = turn else {
            self.accepted_total = Some(total);
            return None;
        };
        let last = usage.get("last").filter(|last| last.is_object())?;
        let input = field(last, "inputTokens", "input_tokens");
        let output = field(last, "outputTokens", "output_tokens");
        let cache_read =
            field(last, "cachedInputTokens", "cached_input_tokens").unwrap_or_default();
        let cache_write =
            field(last, "cacheWriteInputTokens", "cache_write_input_tokens").unwrap_or_default();
        let output_tokens = output.unwrap_or_default();
        let request = RequestUsage {
            segment_id: match child {
                None => format!("codex:{turn}:{total}"),
                Some(child) => format!("codex:{turn}:{child}:{total}"),
            },
            parent: child.map(str::to_owned),
            model: None,
            boundary: UsageBoundary::Request,
            // Codex reports input including the cache read and cache write tokens.
            input_tokens: input
                .unwrap_or_default()
                .saturating_sub(cache_read)
                .saturating_sub(cache_write),
            output_tokens,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            cache_write_1h_tokens: None,
            reasoning_tokens: field(last, "reasoningOutputTokens", "reasoning_output_tokens")
                .map(|reasoning| reasoning.min(output_tokens)),
            cost_usd_reported: None,
            complete: input.is_some() && output.is_some(),
        };
        if request.segment_id.len() > MAX_SEGMENT_ID || !request.is_valid() {
            return None;
        }
        self.accepted_total = Some(total);
        Some(request)
    }
}

/// Build the request segment of one Pi or OMP assistant message from its `usage` object.
///
/// Both harnesses use the pi-ai usage shape, whose `input` already excludes `cacheRead` and
/// `cacheWrite`, so the buckets map one to one. `identity` is the native message id or the
/// per-turn message counter. Returns `None` when `message` has no usage, all token buckets are
/// zero (an aborted message), or the segment would be invalid.
pub fn pi_request(
    provider: &str,
    turn: &str,
    identity: &str,
    message: &Value,
) -> Option<RequestUsage> {
    let usage = message.get("usage").filter(|usage| usage.is_object())?;
    let input = usage["input"].as_u64();
    let output = usage["output"].as_u64();
    let cache_read = usage["cacheRead"].as_u64().unwrap_or_default();
    let cache_write = usage["cacheWrite"].as_u64().unwrap_or_default();
    let output_tokens = output.unwrap_or_default();
    let input_tokens = input.unwrap_or_default();
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    let request = RequestUsage {
        segment_id: format!("{provider}:{turn}:{identity}"),
        parent: None,
        model: message["model"]
            .as_str()
            .filter(|model| !model.is_empty() && model.len() <= MAX_MODEL)
            .map(str::to_owned),
        boundary: UsageBoundary::Request,
        input_tokens,
        output_tokens,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        cache_write_1h_tokens: None,
        reasoning_tokens: usage["reasoning"]
            .as_u64()
            .map(|reasoning| reasoning.min(output_tokens)),
        cost_usd_reported: usage["cost"]["total"]
            .as_f64()
            .filter(|cost| cost.is_finite() && *cost >= 0.0),
        complete: input.is_some() && output.is_some(),
    };
    (request.segment_id.len() <= MAX_SEGMENT_ID && request.is_valid()).then_some(request)
}

/// Native response id usable inside a segment identity, if `message` carries one.
#[must_use]
pub fn response_id(message: &Value) -> Option<&str> {
    message["responseId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= MAX_RESPONSE_ID)
}

/// Read a counter that Codex reports in either camel or snake case.
fn field(value: &Value, camel: &str, snake: &str) -> Option<u64> {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_u64)
}

#[cfg(test)]
mod tests;
