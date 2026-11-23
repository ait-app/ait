//! In-memory usage ledger that records every queued write for assertions.

use std::sync::Mutex;

use crate::ports::usage_ledger::{
    CallBegin, CallFinish, RequestRecord, UsageLedger, UsageLedgerError, UsageQuery, UsageSummary,
};

/// One write in the order the ledger received it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Write {
    /// A `begin_call`.
    Begin(CallBegin),
    /// A `record_request`.
    Request(RequestRecord),
    /// A `finish_call`.
    Finish(CallFinish),
}

/// Ledger that keeps every write in memory and cannot summarize.
#[derive(Debug, Default)]
pub(crate) struct RecordingLedger(Mutex<Vec<Write>>);

impl RecordingLedger {
    /// Every write received so far, in order.
    pub(crate) fn writes(&self) -> Vec<Write> {
        self.0.lock().unwrap().clone()
    }

    /// Every started call, in order.
    pub(crate) fn begins(&self) -> Vec<CallBegin> {
        self.writes()
            .into_iter()
            .filter_map(|write| match write {
                Write::Begin(call) => Some(call),
                Write::Request(_) | Write::Finish(_) => None,
            })
            .collect()
    }

    /// Every recorded request, in order.
    pub(crate) fn requests(&self) -> Vec<RequestRecord> {
        self.writes()
            .into_iter()
            .filter_map(|write| match write {
                Write::Request(request) => Some(request),
                Write::Begin(_) | Write::Finish(_) => None,
            })
            .collect()
    }

    /// Every finished call, in order.
    pub(crate) fn finishes(&self) -> Vec<CallFinish> {
        self.writes()
            .into_iter()
            .filter_map(|write| match write {
                Write::Finish(finish) => Some(finish),
                Write::Begin(_) | Write::Request(_) => None,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl UsageLedger for RecordingLedger {
    fn begin_call(&self, call: CallBegin) {
        self.0.lock().unwrap().push(Write::Begin(call));
    }

    fn record_request(&self, request: RequestRecord) {
        self.0.lock().unwrap().push(Write::Request(request));
    }

    fn finish_call(&self, finish: CallFinish) {
        self.0.lock().unwrap().push(Write::Finish(finish));
    }

    fn recover_interrupted(&self, _ended_at: i64) {}

    async fn summarize(&self, _query: UsageQuery) -> Result<UsageSummary, UsageLedgerError> {
        Err(UsageLedgerError::Unavailable)
    }
}
