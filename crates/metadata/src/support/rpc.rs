//! JSON codecs shared by capability group RPC entry points.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::error::ErrorCode;

/// Decode request parameters into their typed request.
///
/// # Arguments
///
/// * `value` - Raw request parameters taken from the inbound envelope.
///
/// # Returns
/// The decoded request when `value` matches `T`.
///
/// # Errors
///
/// Returns `ErrorCode::InvalidMessage` when `value` does not match `T`, including
/// unknown or missing fields.
pub(crate) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidMessage)
}

/// Encode a typed response into its wire value.
///
/// # Arguments
///
/// * `value` - Typed response payload.
/// * `error` - Failure code reported when the payload cannot be serialized.
///
/// # Returns
/// The serialized JSON value.
///
/// # Errors
///
/// Returns `error` when `value` cannot be represented as JSON.
pub(crate) fn encode(value: impl Serialize, error: ErrorCode) -> Result<Value, ErrorCode> {
    serde_json::to_value(value).map_err(|_| error)
}
