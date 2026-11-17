//! JSON codecs shared by provider RPC entry points.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::rpc::ErrorCode;

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
