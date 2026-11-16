//! Atomic catalog operations for versioned Agent presets and explicit default selection.

use std::fmt::Debug;

use domain::agent::{AgentConfig, AgentSnapshot, AgentTarget, Revision};
use domain::{AgentId, OperationId};

/// Safe Agent failures, with no database diagnostics or credential input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// Invalid configuration, key, version, or page size.
    #[error("invalid Agent parameters or data")]
    Invalid,
    /// No preset has this identity.
    #[error("Agent not found")]
    NotFound,
    /// No immutable revision has this number.
    #[error("Agent revision not found")]
    RevisionNotFound,
    /// A conditional edit observed an obsolete head.
    #[error("Agent revision has changed")]
    RevisionConflict,
    /// Default selection changed after the caller read it.
    #[error("default Agent selection has changed")]
    DefaultConflict,
    /// A disabled preset cannot be selected as default.
    #[error("Agent is disabled")]
    Disabled,
    /// Clear or replace the default before disabling its preset.
    #[error("default Agent must be deselected before disabling it")]
    IsDefault,
    /// The same method/key was already used for different parameters.
    #[error("idempotency key conflicts with a previous request")]
    IdempotencyConflict,
    /// A transient catalog lock is held elsewhere.
    #[error("catalog is busy")]
    Busy,
    /// The database has an unsupported schema or corrupt facts.
    #[error("unsupported catalog format")]
    UnsupportedFormat,
    /// Storage could not complete the operation.
    #[error("Agent I/O failed; retry with the same key")]
    Io,
}

impl From<domain::InvalidValue> for AgentError {
    fn from(_: domain::InvalidValue) -> Self {
        Self::Invalid
    }
}

/// Full replacement command, committed with an immutable revision and receipt.
#[derive(Debug, Clone)]
pub struct ConfigureAgent {
    /// Creation or conditional edit.
    pub(crate) target: AgentTarget,
    /// Validated non-secret fields.
    pub(crate) config: AgentConfig,
    /// Method-scoped durable retry key.
    pub(crate) key: String,
    /// Server time; excluded from the business fingerprint.
    pub(crate) recorded_at: u64,
}

/// Stable completion, which may identify a historical revision after later edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentReceipt {
    /// Durable operation UUID.
    pub(crate) operation_id: OperationId,
    /// Stable preset identity.
    pub(crate) agent_id: AgentId,
    /// Revision published by this operation.
    pub(crate) revision: Revision,
}

/// Explicit global default, initially empty at version zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultSelection {
    /// Selected preset; no implicit first-Agent fallback.
    pub(crate) agent_id: Option<AgentId>,
    /// Monotonic compare-and-swap version within SQLite's integer range.
    pub(crate) version: u64,
}

/// Conditional replacement of the global default.
#[derive(Debug, Clone)]
pub struct SelectDefault {
    /// Desired preset or an explicit clear.
    pub(crate) agent_id: Option<AgentId>,
    /// Observed selection version.
    pub(crate) expected_version: u64,
    /// Durable method-scoped retry key.
    pub(crate) key: String,
}

/// Stable default-change completion; query current selection separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultReceipt {
    /// Durable operation UUID.
    pub(crate) operation_id: OperationId,
    /// Selection as committed by this operation.
    pub(crate) selection: DefaultSelection,
}

impl ConfigureAgent {
    /// Return the creation or conditional replacement target supplied by the service.
    #[must_use]
    pub fn target(&self) -> AgentTarget {
        self.target
    }

    /// Borrow the validated configuration to persist in the new revision.
    #[must_use]
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Borrow the method-scoped retry key to check before current-state preconditions.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Return the service's Unix epoch milliseconds, excluded from retry fingerprints.
    #[must_use]
    pub fn recorded_at(&self) -> u64 {
        self.recorded_at
    }
}

impl AgentReceipt {
    /// Construct a committed receipt from the operation, preset and revision identities.
    #[must_use]
    pub fn new(operation_id: OperationId, agent_id: AgentId, revision: Revision) -> Self {
        Self {
            operation_id,
            agent_id,
            revision,
        }
    }

    /// Return the durable operation identity.
    #[must_use]
    pub fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Return the preset identity committed by this operation.
    #[must_use]
    pub fn agent_id(self) -> AgentId {
        self.agent_id
    }

    /// Return the immutable revision committed by this operation.
    #[must_use]
    pub fn revision(self) -> Revision {
        self.revision
    }
}

impl DefaultSelection {
    /// Construct a selection for `agent_id` at the supplied compare-and-swap `version`.
    /// # Errors
    /// Rejects versions outside the catalog's signed integer range.
    pub fn new(agent_id: Option<AgentId>, version: u64) -> Result<Self, AgentError> {
        if version > i64::MAX as u64 {
            return Err(AgentError::Invalid);
        }
        Ok(Self { agent_id, version })
    }

    /// Return the explicitly selected preset, or `None` for an empty selection.
    #[must_use]
    pub fn agent_id(self) -> Option<AgentId> {
        self.agent_id
    }

    /// Return the compare-and-swap version, including zero for the initial selection.
    #[must_use]
    pub fn version(self) -> u64 {
        self.version
    }
}

impl SelectDefault {
    /// Return the desired preset, or `None` to clear the selection.
    #[must_use]
    pub fn agent_id(&self) -> Option<AgentId> {
        self.agent_id
    }

    /// Return the version the caller observed before requesting the change.
    #[must_use]
    pub fn expected_version(&self) -> u64 {
        self.expected_version
    }

    /// Borrow the method-scoped retry key to check before current-state preconditions.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl DefaultReceipt {
    /// Construct a receipt for `operation_id` and the selection it committed.
    #[must_use]
    pub fn new(operation_id: OperationId, selection: DefaultSelection) -> Self {
        Self {
            operation_id,
            selection,
        }
    }

    /// Return the durable operation identity.
    #[must_use]
    pub fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Return the historical selection committed by this operation.
    #[must_use]
    pub fn selection(self) -> DefaultSelection {
        self.selection
    }
}

/// Blocking catalog port. Every write checks receipts before current-state preconditions.
pub trait AgentCatalog: Debug + Send {
    /// Atomically append a revision, advance the head, and persist a receipt.
    ///
    /// # Errors
    /// Rejects conflicting retries, obsolete revisions, disabling a default, or storage failures.
    fn configure(&mut self, command: &ConfigureAgent) -> Result<AgentReceipt, AgentError>;
    /// Read an exact immutable revision, or the current head when absent.
    ///
    /// # Errors
    /// Returns missing Agent/revision or storage errors.
    fn get_agent(
        &mut self,
        id: AgentId,
        revision: Option<Revision>,
    ) -> Result<AgentSnapshot, AgentError>;
    /// Read current heads by stable ID, at most 50, after an exclusive cursor.
    ///
    /// # Errors
    /// Rejects page sizes outside 1–50 or storage errors.
    fn list_agents(
        &mut self,
        after: Option<AgentId>,
        limit: usize,
    ) -> Result<Vec<AgentSnapshot>, AgentError>;
    /// Read the current explicit default without testing execution or resolving credentials.
    ///
    /// # Errors
    /// Returns storage or invalid-state errors.
    fn get_default(&mut self) -> Result<DefaultSelection, AgentError>;
    /// Atomically change the default and record its receipt, even for a same-value new operation.
    ///
    /// # Errors
    /// Rejects conflicting retries, stale versions, missing/disabled presets, or storage failures.
    fn set_default(&mut self, command: &SelectDefault) -> Result<DefaultReceipt, AgentError>;
}
