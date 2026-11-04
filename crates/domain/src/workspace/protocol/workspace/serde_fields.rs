use serde::{Deserialize, Deserializer};

use super::Mergeable;

/// Require a field to be present while accepting `null` as `None`.
pub(crate) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Distinguish an omitted field from a present one; `null` is invalid.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Accept a positive integral JSON number no larger than [`MAX_SAFE_INTEGER`].
pub(crate) fn positive_integer<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    safe_integer(f64::deserialize(d)?)
}

/// Largest integer every JavaScript client represents exactly (`2^53 - 1`).
pub(crate) const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

fn safe_integer<E: serde::de::Error>(value: f64) -> Result<u64, E> {
    // Formatting an integral value avoids lossy float-to-int casts.
    if value.is_finite()
        && value >= 1.0
        && value.fract() == 0.0
        && let Ok(integer) = format!("{value:.0}").parse::<u64>()
        && integer <= MAX_SAFE_INTEGER
    {
        Ok(integer)
    } else {
        Err(E::custom("expected a positive safe integer"))
    }
}

pub(super) fn optional_positive_integer<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<u64>, D::Error> {
    positive_integer(d).map(Some)
}

pub(super) fn nullable_positive_integer<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<u64>, D::Error> {
    Option::<f64>::deserialize(d)?.map(safe_integer).transpose()
}

pub(super) fn mergeable<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Mergeable>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(Some(
        serde_json::from_value(value).unwrap_or(Mergeable::Unknown),
    ))
}
