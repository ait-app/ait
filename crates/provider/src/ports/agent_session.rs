//! Provider-owned Agent sessions used by the independent server.
//!
//! These contracts mirror the create/resume, runtime inspection, persistence and close portion
//! of Paseo's `AgentClient` and `AgentSession`. A concrete provider adapter owns native execution.

use std::borrow::Cow;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};

/// A sendable provider operation borrowing its client or session.
pub(crate) type AgentSessionFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentSessionError>> + Send + 'a>>;

/// Provider failure safe to expose at the application boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AgentSessionError {
    /// The provider cannot currently start or resume a session.
    #[error("provider is unavailable")]
    Unavailable,
    /// The provider failed an operation or returned inconsistent facts.
    #[error("provider session operation failed")]
    Failed,
    /// The provider rejected a request while the transport remains usable.
    #[error("provider rejected the session operation")]
    Rejected,
    /// The native session is already owned by another interactive writer.
    #[error("provider session is in use")]
    Busy,
    /// A native history read failed with payload-free page and budget context.
    #[error(transparent)]
    History(super::native_history::HistoryError),
}

/// Provider-independent inputs required to construct or resume a native session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentSessionSpec {
    /// Provider identity.
    pub(crate) provider: String,
    /// Native session working directory.
    pub(crate) cwd: String,
    /// Persistable provider configuration.
    pub(crate) config: StoredAgentConfig,
}

/// Whether a resumed native session may accept a new foreground turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentResumePurpose {
    /// Resume an active Agent for interaction.
    Interactive,
    /// Reconcile untouched controls before an immediate configuration patch.
    /// Live-control adapters retain replaced preferences until that patch succeeds.
    Configuration {
        /// The patch replaces the saved model selection.
        replace_model: bool,
        /// The patch explicitly replaces or clears the saved thinking selection.
        replace_thinking: bool,
    },
    /// Read the history of an archived Agent without claiming an interactive writer.
    History,
}

/// Native foreground-turn progress or result. This is not a host Run or Message tree.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AgentTurnEvent {
    /// A native autonomous foreground turn started, for example while pursuing a Codex goal.
    Started(String),
    /// Native child progress or lifecycle, independent of the foreground turn's completion.
    Subagent(super::controls::SubagentEvent),
    /// One incremental display event with a retry-stable observation identity.
    Progress {
        /// Unique identity for this observation, distinct from the native item identity.
        observation: String,
        /// Text delta or tool snapshot, keyed by the eventual completed native item.
        entry: crate::protocol::timeline::NativeItem,
    },
    /// A live native approval; the request payload is never written to display history.
    PermissionRequested(serde_json::Value),
    /// The native provider withdrew or resolved a previously published permission request.
    PermissionResolved(String),
    /// One immutable normalized display item produced by the provider adapter.
    Timeline(crate::protocol::timeline::NativeItem),
    /// Complete authoritative native transcript, reconciled atomically before turn settlement.
    History(Vec<crate::protocol::timeline::NativeItem>),
    /// Complete current usage snapshot; consumers replace rather than sum repeated observations.
    Usage(crate::protocol::usage::AgentUsage),
    /// Native initialization resolved the session's actual model or runtime settings.
    RuntimeInfo(StoredAgentRuntimeInfo),
    /// The native provider drained its turn successfully.
    Completed(Option<String>),
    /// The native provider acknowledged interruption.
    Cancelled,
    /// The native provider failed, exited, or requested unsupported interaction.
    Failed,
}

/// Adapter-owned configuration disposition, independent of runtime snapshot availability.
#[derive(Debug)]
pub(crate) enum ConfigApplication {
    /// The adapter leaves configuration to the host's existing next-turn path.
    Deferred,
    /// The adapter queued durable native intent; persist its configuration and handle.
    Queued,
    /// Native controls completed and returned their confirmed runtime facts.
    Applied(StoredAgentRuntimeInfo),
}

/// Live provider session. Closing releases resources without deleting native history.
pub(crate) trait AgentSession: Debug + Send {
    /// Report a definitive initialization rejection after exposing reconciled native facts.
    /// The manager persists runtime, effective configuration and consumed intent before
    /// returning this error and releasing the session. Other initialization failures must
    /// still fail creation or resume directly.
    fn initialization_error(&self) -> Option<AgentSessionError> {
        None
    }

    /// Attach a connection-owned native audio stream. Unsupported adapters reject it.
    fn start_realtime(
        &mut self,
        _endpoint: model::realtime::Endpoint,
    ) -> AgentSessionFuture<'_, ()> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Whether native audio requires low-latency polling even without a text turn.
    fn realtime_active(&self) -> bool {
        false
    }

    /// Drain bounded microphone input and perform native cancellation before observing events.
    fn service_realtime(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Resolve the session's account before deciding whether a quota read can reuse history.
    /// # Errors
    /// Returns native authentication inspection failures, without exposing credentials.
    fn prepare_account_usage(
        &self,
    ) -> AgentSessionFuture<'_, super::account_usage::PreparedUsage<'_>> {
        let report = self.account_usage();
        Box::pin(async move {
            Ok(super::account_usage::PreparedUsage {
                identity: None,
                report,
            })
        })
    }
    /// Return session-scoped controls when native composition owns the available choices.
    /// `config` includes pending user selections; `None` retains client-level controls.
    fn control_settings(&self, _config: &StoredAgentConfig) -> Option<serde_json::Value> {
        None
    }

    /// Validate a proposed configuration against this session's native constraints.
    /// `config` is the proposed configuration; success permits further catalog validation.
    /// # Errors
    /// Returns rejection before persisting a change that cannot apply to this session.
    fn validate_config_update(&self, _config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
    }

    /// Apply a validated patch immediately when the adapter supports native live controls.
    /// `Deferred` preserves host-only configuration without reading a native handle.
    /// `Queued` persists adapter-owned next-turn intent; `Applied` confirms live controls.
    /// # Errors
    /// Returns native control or readback failures; a partial native change may still be observed.
    fn apply_config<'a>(
        &'a mut self,
        _config: &'a StoredAgentConfig,
        _patch: &'a crate::protocol::agent_config::ConfigPatch,
    ) -> AgentSessionFuture<'a, ConfigApplication> {
        Box::pin(async { Ok(ConfigApplication::Deferred) })
    }

    /// Return adapter-owned durable configuration, including accepted next-turn intent.
    /// `None` preserves the host's requested configuration for other adapters. The manager
    /// reads this during configuration after `Queued`, `Applied`, or a failed operation's
    /// cached runtime observation.
    fn effective_config(&self) -> Option<StoredAgentConfig> {
        None
    }

    /// Take a cached runtime observation without native I/O after an operation fails.
    /// `Some` transfers observation ownership to the manager so shutdown can persist
    /// effective configuration and consumed intent before releasing the session.
    /// `None` leaves adapters without cached observations unchanged.
    fn take_runtime_observation(&mut self) -> Option<StoredAgentRuntimeInfo> {
        None
    }

    /// Return safe classified failure facts before the failed native connection is closed.
    fn failure_diagnostic(&self) -> Option<super::diagnostics::Diagnostic> {
        None
    }
    /// Read plan quota using this session's actual native launch environment.
    ///
    /// Returns safe native account facts, never authentication material.
    /// # Errors
    /// Returns unavailable when the adapter has no session-scoped accounting support.
    fn account_usage(&self) -> AgentSessionFuture<'_, serde_json::Value> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Return the most recent failure's user-visible explanation, if available.
    ///
    /// Adapters must exclude credentials and private diagnostics, and keep the message at most
    /// 4096 bytes. The explanation remains available after closing a failed session.
    fn failure_message(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Native foreground work was accepted but its start notification has not arrived yet.
    fn pending_foreground(&self) -> bool {
        false
    }

    /// Withdraw accepted autonomous work that has not announced a foreground turn yet.
    /// # Errors
    /// Returns native transport failures; an uncertain cancellation is never reported successful.
    fn cancel_pending(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Execute a provider command that does not interrupt or replace the active input turn.
    /// # Errors
    /// Returns rejected before mutation for unsupported commands, or uncertain transport failure.
    fn out_of_band<'a>(
        &'a mut self,
        _prompt: &'a crate::protocol::prompt::AgentPrompt,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async { Err(AgentSessionError::Rejected) })
    }
    /// Current verified native descendants observed by this live session.
    fn subagents(&self) -> Vec<super::controls::NativeSubagent> {
        Vec::new()
    }
    /// Prepare a session-scoped question answer that requires durable user-input admission.
    /// This has no side effects; call `respond_permission` only after admission succeeds.
    /// # Errors
    /// Returns rejected for stale or invalid answers.
    fn prepare_permission_response(
        &self,
        _id: &str,
        _response: &serde_json::Value,
    ) -> Result<Option<crate::protocol::prompt::AgentPrompt>, AgentSessionError> {
        Ok(None)
    }

    /// Configuration explicitly authorized by an approval, applied before its follow-up input.
    /// # Errors
    /// Returns rejected for stale or invalid approval responses.
    fn permission_config_patch(
        &self,
        _id: &str,
        _response: &serde_json::Value,
    ) -> Result<Option<crate::protocol::agent_config::ConfigPatch>, AgentSessionError> {
        Ok(None)
    }

    /// Inspect pending approvals for reconnecting clients; IDs expire with this native session.
    fn pending_permissions(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// Answer one pending approval without extending its authority to later calls.
    /// # Errors
    /// Returns rejected for stale/invalid answers, or a native transport failure.
    fn respond_permission<'a>(
        &'a mut self,
        _id: &'a str,
        _response: &'a serde_json::Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async { Err(AgentSessionError::Rejected) })
    }
    /// Return the provider that owns this session.
    fn provider(&self) -> &str;

    /// Inspect current provider facts after creation or resume.
    ///
    /// # Errors
    /// Returns a provider error when the native runtime cannot be inspected.
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo>;

    /// Describe the native identity required to resume this session, if available.
    fn persistence(&self) -> Option<AgentPersistenceHandle>;

    /// Start one text-only foreground turn and return its native identity.
    ///
    /// # Errors
    /// Returns an error for a busy, history-only, or unsupported session.
    fn start_turn<'a>(
        &'a mut self,
        _text: &'a str,
        _config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Submit one validated rich input, preserving attachment order and output constraints.
    /// # Errors
    /// Legacy implementations reject non-text input instead of silently dropping it.
    fn start_input<'a>(
        &'a mut self,
        prompt: &'a crate::protocol::prompt::AgentPrompt,
        config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async move {
            prompt.validate()?;
            if !prompt.is_plain_text() {
                return Err(AgentSessionError::Rejected);
            }
            self.start_turn(&prompt.text, config).await
        })
    }

    /// Append text to exactly `turn_id`; success means admission at the adapter's native boundary.
    /// Codex acknowledges its RPC; Claude accepts input on its live SDK stream and echoes it later.
    /// # Errors
    /// Returns rejected for unsupported or definitively refused input. Other errors leave
    /// admission uncertain and must never trigger an automatic resubmission.
    fn steer_turn<'a>(
        &'a mut self,
        _turn_id: &'a str,
        _text: &'a str,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async { Err(AgentSessionError::Rejected) })
    }

    /// Steer with complete rich input; unsupported providers must reject without partial admission.
    /// # Errors
    /// Returns definitive rejection or uncertain provider/transport failure.
    fn steer_input<'a>(
        &'a mut self,
        turn_id: &'a str,
        prompt: &'a crate::protocol::prompt::AgentPrompt,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            prompt.validate()?;
            if !prompt.is_plain_text() {
                return Err(AgentSessionError::Rejected);
            }
            self.steer_turn(turn_id, &prompt.text).await
        })
    }

    /// Interrupt the identified native turn without deleting session history.
    ///
    /// # Errors
    /// Returns an error when the provider cannot acknowledge interruption.
    fn cancel_turn<'a>(&'a mut self, _turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Read a queued progress or terminal event without waiting for new provider output.
    ///
    /// # Errors
    /// Returns an error if the provider connection has failed.
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(None)
    }

    /// Release the live native runtime without deleting its durable history.
    ///
    /// # Errors
    /// Returns a provider error if runtime ownership remains uncertain.
    fn close(&mut self) -> AgentSessionFuture<'_, ()>;
}

/// Factory and availability boundary for one independent provider adapter.
pub(crate) trait AgentClient: Debug + Send + Sync {
    /// Return the total catalog budget, including queueing and availability checks.
    fn discovery_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    /// Whether this adapter implements native session listing and import inspection.
    fn supports_session_import(&self) -> bool {
        false
    }

    /// Whether configuration requires a live native session before it can be accepted.
    /// The manager resumes a dormant session before calling its live controls.
    fn supports_live_config(&self) -> bool {
        false
    }

    /// Whether native history can be replayed as a complete authoritative transcript.
    /// Adapters without replay retain the server's already persisted display timeline.
    fn supports_history_replay(&self) -> bool {
        true
    }

    /// Create a session with ephemeral environment overrides for its native child process.
    ///
    /// Default adapters accept an empty environment and reject unsupported nonempty values.
    /// # Errors
    /// Returns validation or provider errors without persisting environment values.
    fn create_session_with_environment<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        environment: &'a super::environment::AgentEnvironment,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            if !environment.is_empty() {
                return Err(AgentSessionError::Rejected);
            }
            self.create_session(spec).await
        })
    }

    /// Whether this adapter implements an isolated auxiliary generation channel.
    fn supports_summary_generation(&self) -> bool {
        false
    }

    /// Select this adapter's auxiliary model from its discovered model catalog.
    ///
    /// Returns `None` when no suitable small model is available. Explicit user model
    /// overrides are resolved separately and do not inherit the foreground model.
    fn summary_model(
        &self,
        _models: &[serde_json::Value],
    ) -> Option<domain::summary::SummarySelection> {
        None
    }

    /// Run an isolated, tool-disabled structured request without registering a foreground Agent.
    /// Any temporary native history is cleaned up by the adapter.
    /// `spec` selects cwd/model/reasoning; `prompt` is source-only wording and `schema` its output.
    /// # Errors
    /// Returns unavailable for unsupported providers, or safe protocol/timeout failures.
    fn generate_summary<'a>(
        &'a self,
        _spec: &'a AgentSessionSpec,
        _prompt: &'a str,
        _schema: &'a serde_json::Value,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Whether this text names a provider command that bypasses foreground input scheduling.
    fn handles_out_of_band(&self, _text: &str) -> bool {
        false
    }
    /// Restore durable session-scoped questions without opening a native writer.
    /// Native tool approvals are deliberately excluded because their transport has expired.
    fn persisted_permissions(&self, _handle: &AgentPersistenceHandle) -> Vec<serde_json::Value> {
        Vec::new()
    }
    /// Describe selectable modes/features and implemented native control flags without I/O.
    fn settings(&self, _config: &StoredAgentConfig) -> serde_json::Value {
        serde_json::json!({"availableModes":[],"features":[],"capabilities":{}})
    }

    /// Inspect the proposed configuration's features without registering a durable Agent.
    ///
    /// `spec` supplies a normalized directory and draft settings. Adapters may perform bounded
    /// capability discovery, but must not submit user input or retain a native writer.
    /// # Errors
    /// Returns provider unavailability or a malformed capability response.
    fn draft_features<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Vec<serde_json::Value>> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Err(AgentSessionError::Unavailable);
            }
            serde_json::from_value(self.settings(&spec.config)["features"].take())
                .map_err(|_| AgentSessionError::Failed)
        })
    }
    /// Validate dynamic selections before committing configuration.
    /// # Errors
    /// Returns unsupported settings or unavailable native discovery.
    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move { self.validate_config(&spec.config) })
    }

    /// Validate a proposed patch; providers may avoid rediscovering unchanged selections.
    /// `spec` contains the resulting configuration, and `patch` identifies explicit changes.
    /// Returns the same validation errors as selection validation by default.
    fn validate_config_patch<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        _patch: &'a crate::protocol::agent_config::ConfigPatch,
    ) -> AgentSessionFuture<'a, ()> {
        self.validate_selection(spec)
    }

    /// Inspect launch and authentication availability without exposing credentials.
    /// Unavailable catalog snapshots may display this guidance; keep it within 4096 bytes.
    /// # Errors
    /// Returns unsupported diagnostics or an inspection failure.
    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Read native account quota facts, not local estimates of token consumption.
    /// # Errors
    /// Returns unsupported accounting or a native query failure.
    fn usage(&self) -> AgentSessionFuture<'_, serde_json::Value> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Resolve the host's account before deciding whether its quota read can reuse history.
    /// # Errors
    /// Returns native authentication inspection failures, without exposing credentials.
    fn prepare_usage(&self) -> AgentSessionFuture<'_, super::account_usage::PreparedUsage<'_>> {
        let report = self.usage();
        Box::pin(async move {
            Ok(super::account_usage::PreparedUsage {
                identity: None,
                report,
            })
        })
    }

    /// List commands and skills available in a validated working directory.
    /// # Errors
    /// Returns malformed discovery or unavailable native support.
    fn commands<'a>(
        &'a self,
        _spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Vec<serde_json::Value>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Discover native child-parent links without registering host Agents.
    /// # Errors
    /// Returns unavailable or incomplete native discovery.
    fn subagents<'a>(
        &'a self,
        _cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<super::controls::NativeSubagent>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Derive a new native history before the selected user message, preserving the old thread.
    /// # Errors
    /// Returns an invalid target, active history, or native fork/rollback failure.
    fn rewind<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _spec: &'a AgentSessionSpec,
        _message_id: &'a str,
    ) -> AgentSessionFuture<'a, super::native_history::SessionHistory> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Restore native tracked-file checkpoints at a verified user message, without rewriting history.
    /// # Errors
    /// Returns unsupported capability, an invalid checkpoint, or a native restore failure.
    fn rewind_files<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _spec: &'a AgentSessionSpec,
        _message_id: &'a str,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }
    /// Return the provider identity served by this client.
    fn provider(&self) -> &str;

    /// Validate persisted next-turn configuration without launching or mutating a session.
    ///
    /// # Errors
    /// Returns an error for unsupported settings. Validation does not verify model availability.
    fn validate_config(&self, _config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    /// Check whether the provider can launch sessions now.
    ///
    /// # Errors
    /// Returns a provider error if availability cannot be determined.
    fn is_available(&self) -> AgentSessionFuture<'_, bool>;

    /// Discover native models and the modes/features supported by this adapter in `cwd`.
    /// # Errors
    /// Returns unavailable or safe native discovery failures.
    fn discover<'a>(
        &'a self,
        _cwd: &'a str,
    ) -> AgentSessionFuture<'a, crate::protocol::provider::Details> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Read existing native history without resuming or claiming an interactive writer.
    /// # Errors
    /// Returns unavailable, missing-history or malformed-provider errors.
    fn history<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<crate::protocol::timeline::NativeItem>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Discover existing sessions without resuming a writer or submitting input.
    /// # Errors
    /// Returns unsupported discovery, malformed native responses or transport failures.
    fn list_sessions<'a>(
        &'a self,
        _options: &'a super::native_history::ListOptions,
    ) -> AgentSessionFuture<'a, Vec<super::native_history::SessionDescriptor>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Inspect one complete native history and configuration without claiming a writer.
    /// # Errors
    /// Returns unavailable history, mismatched identity, partial history or transport failures.
    fn inspect_session<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _cwd: &'a str,
    ) -> AgentSessionFuture<'a, super::native_history::SessionHistory> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    /// Create a new native session using `spec`.
    ///
    /// # Errors
    /// Returns a provider error when construction fails.
    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>>;

    /// Resume the native session identified by `handle` for `purpose`.
    ///
    /// # Errors
    /// Returns a provider error when resume fails or the handle is no longer valid.
    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>>;
}

#[cfg(test)]
mod tests;
