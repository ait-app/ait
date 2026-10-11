//! Encoded response size checks against the connection output budget.

use std::io::{self, Write};

use serde::Serialize;

/// Encoded body budget, reserving room for envelopes and other in-flight output.
pub(crate) const OUTPUT_BUDGET: usize = model::server::MAX_QUEUE_BYTES - 64 * 1024;

/// Whether `value` fits the encoded body budget, stopping at the limit.
pub(crate) fn fits(value: &impl Serialize) -> bool {
    serde_json::to_writer(
        &mut EncodedSize {
            bytes: 0,
            limit: OUTPUT_BUDGET,
        },
        value,
    )
    .is_ok()
}

/// Count the JSON bytes of `value` without allocating its encoded body.
///
/// # Errors
/// Returns the serializer's error if `value` cannot be encoded as JSON.
pub(crate) fn encoded_size(value: &impl Serialize) -> serde_json::Result<usize> {
    let mut counter = EncodedSize {
        bytes: 0,
        limit: usize::MAX,
    };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.bytes)
}

struct EncodedSize {
    bytes: usize,
    limit: usize,
}

impl Write for EncodedSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > self.limit {
            return Err(io::Error::other("Response exceeds the output budget"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
