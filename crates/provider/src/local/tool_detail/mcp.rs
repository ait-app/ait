//! Bounded MCP display fields; complete values remain in the native transcript.

use std::io::{self, Write};

use serde_json::{Value, json};

const INPUT_BYTES: usize = 64 * 1024;
const OUTPUT_BYTES: usize = 512 * 1024;
const MARKER: &str = "\n[Output truncated; full output remains in the native transcript.]";

/// Project MCP arguments and results, retaining JSON structure within each display budget.
///
/// `native` supplies the original tool fields. Returns an unknown-tool detail with bounded
/// input and output, using marked text previews only for fields exceeding their budgets.
pub(super) fn detail(native: &Value) -> Value {
    json!({"type":"unknown","input":bounded(&native["arguments"], INPUT_BYTES),
        "output":bounded(&native["result"], OUTPUT_BYTES)})
}

/// Retain a native MCP error within the same budget as its arguments.
///
/// `value` is the original error. Returns its JSON structure or a marked text preview.
pub(super) fn error(value: &Value) -> Value {
    bounded(value, INPUT_BYTES)
}

fn bounded(value: &Value, limit: usize) -> Value {
    // A serialized JSON prefix can double in size when encoded again as a string.
    let prefix_limit = ((limit - MARKER.len() - 2) / 2).min(super::PREVIEW_BYTES);
    let mut preview = JsonPreview {
        bytes: Vec::new(),
        size: 0,
        limit,
        prefix_limit,
    };
    if serde_json::to_writer(&mut preview, value).is_ok() {
        return value.clone();
    }
    let text = match String::from_utf8(preview.bytes) {
        Ok(text) => text,
        Err(error) => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).expect("serialized JSON prefix contains valid UTF-8")
        }
    };
    json!(format!("{text}{MARKER}"))
}

struct JsonPreview {
    bytes: Vec<u8>,
    size: usize,
    limit: usize,
    prefix_limit: usize,
}

impl Write for JsonPreview {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let retained = bytes.len().min(self.prefix_limit - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..retained]);
        self.size = self.size.saturating_add(bytes.len());
        if self.size > self.limit {
            return Err(io::ErrorKind::WriteZero.into());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
