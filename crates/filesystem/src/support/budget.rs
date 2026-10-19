//! Encoded response size checks against the connection output budget.

use std::io::{self, Write};

use serde::Serialize;

// Every lane of a connection shares one MAX_QUEUE_BYTES output budget, and a small response that
// cannot reserve bytes fails the whole connection. A diff or directory listing may therefore take
// at most a quarter of it, so the other lanes keep room for their own responses while it is written.
pub(crate) const OUTPUT_BUDGET: usize = model::server::MAX_QUEUE_BYTES / 4 - 64 * 1024;

pub(crate) fn fits(value: &impl Serialize) -> bool {
    serde_json::to_writer(&mut EncodedSize::default(), value).is_ok()
}

#[derive(Default)]
struct EncodedSize(usize);

impl Write for EncodedSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > OUTPUT_BUDGET {
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
