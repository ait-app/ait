//! Canonical token management messages; registration is an uncorrelated event.

use serde::Deserialize;

/// Token registration/revocation payload. Debug is omitted to prevent accidental disclosure.
#[derive(Deserialize)]
pub(crate) struct TokenRequest {
    /// Opaque provider token; whitespace is normalized by the lease service.
    pub(crate) token: String,
}
