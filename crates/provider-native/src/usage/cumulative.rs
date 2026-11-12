//! Cumulative provider token counters turned into additive deltas.
//!
//! Running totals are differenced, never summed. A provider restart is detected as a drop in any
//! counter; its first total only becomes the new baseline.

/// Disjoint token buckets of one usage observation, matching `RequestUsage`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageCounters {
    /// Input tokens, excluding cache read and cache write tokens.
    pub input: u64,
    /// Output tokens, including reasoning tokens.
    pub output: u64,
    /// Cache read tokens.
    pub cache_read: u64,
    /// Cache write tokens.
    pub cache_write: u64,
}

/// Baseline of one cumulative usage stream, such as one native conversation.
#[derive(Debug, Default)]
pub struct CumulativeUsage {
    baseline: Option<UsageCounters>,
}

impl CumulativeUsage {
    /// Accept the stream's next running total and return the growth since the last one.
    ///
    /// Returns `None` for the first total, an unchanged total, or a drop in any counter; each of
    /// these leaves `current` as the new baseline.
    pub fn observe(&mut self, current: UsageCounters) -> Option<UsageCounters> {
        let previous = self.baseline.replace(current)?;
        let delta = UsageCounters {
            input: current.input.checked_sub(previous.input)?,
            output: current.output.checked_sub(previous.output)?,
            cache_read: current.cache_read.checked_sub(previous.cache_read)?,
            cache_write: current.cache_write.checked_sub(previous.cache_write)?,
        };
        (delta != UsageCounters::default()).then_some(delta)
    }
}

#[cfg(test)]
mod tests;
