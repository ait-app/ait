//! Safe provider diagnostics shared by adapters and session coordination.

/// Stable failure classes; native strings and credentials never cross this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    /// Credentials are invalid or authentication has expired.
    Authentication,
    /// The native service denied the requested access.
    Permission,
    /// The native service is throttling requests.
    RateLimit,
    /// The account has insufficient quota or credits.
    Quota,
    /// The selected model is overloaded.
    Capacity,
    /// The selected model is unavailable or unsupported.
    Model,
    /// The conversation exceeds the model context budget.
    ContextLimit,
    /// The native service connection could not be established or maintained.
    Network,
    /// The upstream response stream ended without a completion event.
    IncompleteResponse,
    /// A bounded operation exceeded its response deadline.
    Timeout,
    /// The native process or stream ended unexpectedly.
    Closed,
    /// The native response violated its transport or session contract.
    Protocol,
    /// The native process could not be launched.
    Unavailable,
    /// The cause was not recognized safely.
    Unknown,
}

impl Category {
    /// Return the fixed user-facing explanation for this class.
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Authentication => {
                "Provider authentication failed. Sign in again before retrying."
            }
            Self::Permission => "The provider denied access. Check your account or permissions.",
            Self::RateLimit => "The provider is rate limiting requests. Please try again later.",
            Self::Quota => "Provider quota is exhausted. Check your account usage before retrying.",
            Self::Capacity => "Selected model is at capacity. Please try a different model.",
            Self::Model => "The selected model is unavailable. Please choose another model.",
            Self::ContextLimit => {
                "The conversation exceeds the model's context limit. Compact it or start a new conversation."
            }
            Self::Network => "The provider connection failed. Check your network before retrying.",
            Self::IncompleteResponse => {
                "The provider response stream ended before completion. Retry or check the provider service."
            }
            Self::Timeout => "The provider did not respond before the operation timed out.",
            Self::Closed => {
                "The provider process or connection closed before the operation completed."
            }
            Self::Protocol => "The provider returned an invalid or inconsistent response.",
            Self::Unavailable => {
                "The provider could not start. Check that its native application is installed and available."
            }
            Self::Unknown => "Provider execution failed",
        }
    }

    /// Return the stable structured-log classification.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Permission => "permission",
            Self::RateLimit => "rate_limit",
            Self::Quota => "quota",
            Self::Capacity => "model_capacity",
            Self::Model => "model_unavailable",
            Self::ContextLimit => "context_limit",
            Self::Network => "network",
            Self::IncompleteResponse => "incomplete_response",
            Self::Timeout => "timeout",
            Self::Closed => "connection_closed",
            Self::Protocol => "protocol",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// Bounded diagnostic facts. Text is generated from a class, never copied from native payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    /// Common classification used for logs and user-facing text.
    pub(crate) category: Category,
    /// Adapter-owned fixed source label, never native input.
    pub(crate) source: &'static str,
    /// Numeric native RPC error code, when supplied.
    pub(crate) native_code: Option<i64>,
    /// Numeric upstream HTTP status, when supplied.
    pub(crate) http_status: Option<u16>,
    /// `None` means the provider did not supply an authoritative retry state.
    pub(crate) will_retry: Option<bool>,
}

impl Diagnostic {
    /// Construct a diagnostic without assuming native codes or retry behavior.
    pub(crate) const fn new(category: Category, source: &'static str) -> Self {
        Self {
            category,
            source,
            native_code: None,
            http_status: None,
            will_retry: None,
        }
    }
}
