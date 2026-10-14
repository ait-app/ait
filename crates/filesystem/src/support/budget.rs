//! Encoded response size checks against the connection output budget.

use std::io::{self, Write};

use serde::Serialize;

// Leave room for the response/event envelope and other in-flight output.
const OUTPUT_BUDGET: usize = model::server::MAX_QUEUE_BYTES - 64 * 1024;

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
