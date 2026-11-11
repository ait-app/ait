//! Feature-typed Goal capability; adapters own native interpretation and pause semantics.

use std::borrow::Cow;

use domain::agent_runtime::goal::GoalContinuationOwner;

/// Verified effect of the provider's native Pause operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalPauseSemantics {
    /// Disarms future pursuit while allowing the current turn to finish.
    DeferredFinish,
    /// Aborts the current turn and therefore cannot implement uniform Ait Pause.
    AbortInFlight,
    /// Uniform Pause has not been established for this installed version.
    Unknown,
}

/// Whether the native Goal has reliable backend meter facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalMetering {
    /// No reliable native Goal meter is available.
    None,
    /// Native meter facts are available for an actual native run, never a fallback estimate.
    Reliable,
}

/// Adapter-verified eligibility, not provider-name or method-advertisement inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoalCapability {
    /// Native control is usable only together with verified `DeferredFinish` Pause.
    pub(crate) native: bool,
    /// Owner of native continuation when native strategy is selected.
    pub(crate) continuation: GoalContinuationOwner,
    /// Verified native Pause semantics.
    pub(crate) pause: GoalPauseSemantics,
    /// Whether an actual scoped checkpoint tool is injected for fallback turns.
    pub(crate) checkpoint_tool: bool,
    /// Native-only meter availability; never a fallback meter or a hard-cap claim.
    pub(crate) metering: GoalMetering,
    /// Version/capability explanation retained on the host run.
    pub(crate) reason: Option<Cow<'static, str>>,
}

impl Default for GoalCapability {
    fn default() -> Self {
        Self {
            native: false,
            continuation: GoalContinuationOwner::Host,
            pause: GoalPauseSemantics::Unknown,
            checkpoint_tool: false,
            metering: GoalMetering::None,
            reason: Some(Cow::Borrowed(
                "Native Goal control is not available from this adapter",
            )),
        }
    }
}
