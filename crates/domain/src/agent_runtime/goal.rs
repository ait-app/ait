//! Host-owned Goal lifecycle and checkpoint facts, independent of provider transports.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Execution strategy selected once when a run is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStrategy {
    /// A verified native Goal controller owns continuation.
    Native,
    /// Ait continues the existing main writer through its durable inbox.
    Fallback,
}

/// Durable Goal state. A user Stop is Paused with a separate stop reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    /// Pursuit is enabled; pending checkpoint work may still require attention.
    Active,
    /// Further continuation is disarmed without destroying the objective.
    Paused,
    /// Pursuit needs user intervention.
    Blocked,
    /// An authoritative checkpoint or native fact confirmed completion.
    Completed,
    /// The user explicitly removed the Goal.
    Cleared,
    /// Pursuit failed and must not be automatically resent.
    Failed,
}

/// Explicit user operation on the current Goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalControlOp {
    /// Let the current turn finish, but disarm continuation.
    Pause,
    /// Explicitly rearm pursuit and clear a prior user-stop fact.
    Resume,
    /// Cancel the current turn and preserve the paused objective.
    Stop,
    /// Explicitly remove the Goal, distinct from successful completion.
    Clear,
}

/// Who drives continuation for the selected strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalContinuationOwner {
    /// Existing Ait input FIFO.
    #[default]
    Host,
    /// Provider-native RPC execution lifecycle.
    NativeRpc,
    /// Provider-owned autonomous driver.
    NativeDriver,
}

/// Provider-projected native facts; consumers do not interpret provider JSON statuses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeGoalFacts {
    /// Adapter-normalized state, not a host guess from the status string.
    pub state: GoalState,
    /// Native objective, needed for generation reconciliation.
    pub objective: String,
    /// Native identity when the provider supplies one.
    pub native_id: Option<String>,
    /// Original provider status for read-only presentation.
    pub native_status: String,
    /// Provider revision; adapters own ordering and stale-result rejection.
    pub revision: Option<String>,
    /// Reliable native meter facts only; absent remains unavailable.
    pub meters: Option<Value>,
}

/// Structured verdict accepted only from the current Goal checkpoint's owning turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerdict {
    /// Progress requires another continuation.
    Continue,
    /// The objective is complete according to the supplied evidence.
    Complete,
    /// Pursuit cannot continue without intervention.
    Blocked,
}

/// Last validated checkpoint, attributed to an actual Goal-owned turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCheckpoint {
    /// Per-run counter, starting at one and advancing only on accepted Continue.
    pub nonce: u64,
    /// Native turn that supplied this checkpoint.
    pub turn_id: String,
    /// Structured verdict, never inferred from completion keywords.
    pub verdict: GoalVerdict,
    /// Evidence supplied by the main writer.
    pub evidence: String,
    /// Next action supplied by the main writer.
    pub next_action: String,
}

/// Actual durable inbox receipt state, including queued versus submitted distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalReceiptState {
    /// Reserved but not claimed by the writer.
    Queued,
    /// Claimed; outcome is not yet durable.
    Claimed,
    /// The provider accepted the input.
    Accepted,
    /// Outcome is unknown and must not be automatically resent.
    Uncertain,
    /// The provider rejected the input.
    Rejected,
    /// An unsubmitted input was cancelled.
    Cancelled,
}

/// Link to the existing durable input receipt, not a second receipt store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContinuation {
    /// Existing `input_receipts` message identity.
    pub message: String,
    /// Checkpoint nonce owned by this input.
    pub nonce: u64,
    /// Zero for the initial submission; distinct explicit repair/rearm attempts avoid collisions.
    pub attempt: u32,
    /// Observed state of the durable inbox receipt.
    pub state: GoalReceiptState,
    /// Turn accepted for this input, when known.
    pub turn_id: Option<String>,
}

/// Why the user explicitly stopped pursuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStopKind {
    /// Explicit user Stop, not Pause or successful completion.
    UserStopped,
}

/// Durable distinction between Pause and Stop, preserved through native reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalStopReason {
    /// Typed user-stop fact.
    pub kind: GoalStopKind,
    /// Timestamp of the accepted user operation.
    pub at: String,
    /// Existing durable control receipt identity.
    pub receipt: Option<String>,
}

/// One host-owned run. Native resume handles remain owned by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRun {
    /// Host run identity, never substituted for a native identity.
    pub goal_id: String,
    /// Monotonic version checked by explicit control requests.
    pub version: u64,
    /// Fixed execution strategy.
    pub strategy: GoalStrategy,
    /// Capability/version explanation for the selected strategy.
    pub strategy_reason: Option<String>,
    /// User-confirmed objective.
    pub objective: String,
    /// Durable lifecycle state.
    pub state: GoalState,
    /// Fixed continuation owner for the run.
    pub continuation_owner: GoalContinuationOwner,
    /// Latest native turn attributed to this Goal, not an unrelated user turn.
    pub recent_turn: Option<String>,
    /// Latest adapter-projected native facts, for Native strategy only.
    pub native: Option<NativeGoalFacts>,
    /// Current checkpoint nonce.
    pub nonce: u64,
    /// Last accepted checkpoint.
    pub checkpoint: Option<GoalCheckpoint>,
    /// Consecutive structurally unchanged checkpoints.
    pub no_progress: u8,
    /// Whether this run consumed its single malformed-checkpoint repair.
    pub repair_used: bool,
    /// A repair is due but has not passed the user-priority admission boundary.
    pub repair_pending: bool,
    /// Awaiting a valid checkpoint; restart must not automatically pump another turn.
    pub pending: bool,
    /// Link to the current or most recent durable input receipt.
    pub continuation: Option<GoalContinuation>,
    /// Visible reason for blocked or interrupted pursuit.
    pub blocked_reason: Option<String>,
    /// Stop presentation survives native get returning Paused.
    pub stop_reason: Option<GoalStopReason>,
    /// Run creation timestamp.
    pub created_at: String,
    /// Latest state change timestamp.
    pub updated_at: String,
}
