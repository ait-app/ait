//! Usage ledger hooks of ADR-246: one call per provider turn, its requests and its outcome.
//!
//! Every write is queued by the ledger and never fails or delays the turn.

use std::sync::Arc;

use domain::agent_runtime::{
    PersistedAgentRuntimeRecord, StoredAgentConfig, StoredAgentRuntimeInfo,
};

use crate::ports::agent_session::AgentTurnEvent;
use crate::ports::usage_ledger::{
    CallBegin, CallFinish, CallOutcome, CallSource, RequestRecord, RequestUsage, UsageLedger,
};
use crate::support::time::now_unix_ms;

/// Label that a schedule puts on every Agent it creates.
const SCHEDULE_LABEL: &str = "paseo.schedule-id";

/// One Agent's handle on the host ledger and the provider call that is open for its turn.
#[derive(Debug)]
pub(super) struct Calls {
    /// Host ledger that receives every write.
    ledger: Arc<dyn UsageLedger>,
    /// Call of the active native turn; its id is the turn id.
    active: Option<String>,
}

impl Calls {
    /// Track calls of one Agent in `ledger`, with no call open.
    pub(super) fn new(ledger: Arc<dyn UsageLedger>) -> Self {
        Self {
            ledger,
            active: None,
        }
    }

    /// Open `call_id` for the Agent of `record` as the active call, started now by `source`.
    ///
    /// `model` is the model the turn starts with, if known.
    pub(super) fn begin(
        &mut self,
        call_id: String,
        record: &PersistedAgentRuntimeRecord,
        model: Option<String>,
        source: CallSource,
    ) {
        self.ledger
            .begin_call(call_begin(call_id.clone(), record, model, source));
        self.active = Some(call_id);
    }

    /// Open the call of `turn`, which host input to the Agent of `record` just started.
    ///
    /// `runtime` is the session's state after the start and `config` the turn's configuration;
    /// together they name the model.
    pub(super) fn begin_input(
        &mut self,
        turn: String,
        record: &PersistedAgentRuntimeRecord,
        runtime: Option<&StoredAgentRuntimeInfo>,
        config: &StoredAgentConfig,
    ) {
        self.begin(turn, record, model(runtime, Some(config)), source(record));
    }

    /// Record host input that the provider refused to start as a finished call without requests.
    ///
    /// The call gets a unique id because a refused start has no turn id. `runtime`, else the
    /// runtime state of `record`, and `config` name the model the turn would have used;
    /// `token_usage_available` is the session's capability.
    pub(super) fn reject(
        &self,
        record: &PersistedAgentRuntimeRecord,
        runtime: Option<&StoredAgentRuntimeInfo>,
        config: &StoredAgentConfig,
        token_usage_available: bool,
    ) {
        let call_id = format!("rejected:{}:{}", record.id, uuid::Uuid::new_v4());
        let model = model(runtime.or(record.runtime_info.as_ref()), Some(config));
        self.ledger
            .begin_call(call_begin(call_id.clone(), record, model, source(record)));
        self.ledger.finish_call(CallFinish {
            call_id,
            outcome: CallOutcome::Rejected,
            ended_at: now_unix_ms(),
            token_usage_available,
        });
    }

    /// Attribute `usage` reported by Agent `agent_id` to the active call.
    ///
    /// Usage that arrives while no call is open, such as a late frame between turns, is dropped.
    pub(super) fn record(&self, agent_id: &str, usage: RequestUsage) {
        let Some(call_id) = &self.active else {
            tracing::debug!(agent_id, "Request usage without an active call was dropped");
            return;
        };
        self.ledger.record_request(RequestRecord {
            call_id: call_id.clone(),
            agent_id: agent_id.to_owned(),
            recorded_at: now_unix_ms(),
            usage,
        });
    }

    /// Finish the active call, if any, with the terminal `outcome`.
    ///
    /// `token_usage_available` is whether the session reports token usage at all.
    pub(super) fn finish(&mut self, outcome: CallOutcome, token_usage_available: bool) {
        if let Some(call_id) = self.active.take() {
            self.ledger.finish_call(CallFinish {
                call_id,
                outcome,
                ended_at: now_unix_ms(),
                token_usage_available,
            });
        }
    }
}

/// Finish the active call of `agent` with the outcome of its pending terminal event.
///
/// `poll` reaches its terminal branch only after `drain` stopped at a terminal event, so every
/// other pending state is a host invariant violation; it is logged and counts as completed,
/// matching how `poll` settles it.
pub(super) fn finish_turn(agent: &mut super::LiveAgent) {
    let outcome = match &agent.pending {
        Some(AgentTurnEvent::Completed(_)) => CallOutcome::Completed,
        Some(AgentTurnEvent::Failed) => CallOutcome::Failed,
        Some(AgentTurnEvent::Cancelled) => CallOutcome::Cancelled,
        Some(
            AgentTurnEvent::Started(_)
            | AgentTurnEvent::RuntimeInfo(_)
            | AgentTurnEvent::Usage(_)
            | AgentTurnEvent::RequestUsage(_)
            | AgentTurnEvent::Subagent(_)
            | AgentTurnEvent::PermissionRequested(_)
            | AgentTurnEvent::PermissionResolved(_)
            | AgentTurnEvent::Progress { .. }
            | AgentTurnEvent::Timeline(_)
            | AgentTurnEvent::History(_),
        )
        | None => {
            tracing::warn!(agent_id = %agent.record.id,
                "Ledger call finished without a terminal provider event");
            CallOutcome::Completed
        }
    };
    agent
        .calls
        .finish(outcome, agent.session.reports_token_usage());
}

/// Finish the active call of `agent`, if any, because the host closed its session mid-turn.
pub(super) fn interrupt(agent: &mut super::LiveAgent) {
    agent.calls.finish(
        CallOutcome::Interrupted,
        agent.session.reports_token_usage(),
    );
}

/// Start record of call `call_id` for the Agent of `record`, started now.
fn call_begin(
    call_id: String,
    record: &PersistedAgentRuntimeRecord,
    model: Option<String>,
    source: CallSource,
) -> CallBegin {
    CallBegin {
        call_id,
        agent_id: record.id.clone(),
        provider: record.provider.clone(),
        model,
        source,
        workspace_id: record.workspace_id.clone(),
        started_at: now_unix_ms(),
    }
}

/// Source of host input to the Agent of `record`.
///
/// # Returns
/// `Bot` for a Bot's internal session, `Schedule` for an Agent a schedule created, else `User`.
/// A heartbeat prompts an existing Agent and leaves no mark on its record, so its input takes
/// that Agent's source.
pub(super) fn source(record: &PersistedAgentRuntimeRecord) -> CallSource {
    if record.internal && record.owner.is_some() {
        CallSource::Bot
    } else if record.labels.contains_key(SCHEDULE_LABEL) {
        CallSource::Schedule
    } else {
        CallSource::User
    }
}

/// Model a call starts with: the provider-reported `runtime` model, else the `config` model.
pub(super) fn model(
    runtime: Option<&StoredAgentRuntimeInfo>,
    config: Option<&StoredAgentConfig>,
) -> Option<String> {
    runtime
        .and_then(|info| info.model.clone())
        .or_else(|| config.and_then(|config| config.model.clone()))
}
