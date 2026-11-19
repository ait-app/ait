//! Version-one creation intent hashing, independent of JSON formatting and object insertion order.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Hash `intent` with type tags and lengths, preserving JSON value equality.
/// Object keys are sorted, array order is significant, and floating-point negative zero is
/// normalized. Large strings feed the digest directly without another serialized copy.
#[must_use]
pub fn of(intent: &Value) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"ait-creation-intent-v1\0");
    hash(&mut digest, intent);
    digest.finalize().into()
}

fn bytes(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn hash(digest: &mut Sha256, value: &Value) {
    match value {
        Value::Null => digest.update([0]),
        Value::Bool(value) => digest.update([1, u8::from(*value)]),
        Value::Number(value) => {
            if let Some(n) = value.as_u64() {
                digest.update([2, 0]);
                digest.update(n.to_be_bytes());
            } else if let Some(n) = value.as_i64() {
                digest.update([2, 1]);
                digest.update(n.to_be_bytes());
            } else {
                let n = value.as_f64().expect("JSON number is an integer or float");
                digest.update([2, 2]);
                digest.update(if n == 0.0 { 0.0 } else { n }.to_be_bytes());
            }
        }
        Value::String(value) => {
            digest.update([3]);
            bytes(digest, value.as_bytes());
        }
        Value::Array(values) => {
            digest.update([4]);
            digest.update((values.len() as u64).to_be_bytes());
            for value in values {
                hash(digest, value);
            }
        }
        Value::Object(values) => {
            digest.update([5]);
            digest.update((values.len() as u64).to_be_bytes());
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (key, value) in entries {
                bytes(digest, key.as_bytes());
                hash(digest, value);
            }
        }
    }
}

#[cfg(test)]
mod tests;
