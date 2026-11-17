//! Bounded newline-delimited JSON frames read from a native process stream.

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use crate::diagnostics::Diagnostics;
use provider_core::ports::diagnostics::{Category, Diagnostic};

/// One decoded frame and the bytes it occupied on the stream, including its newline.
#[derive(Debug)]
pub struct Frame {
    /// Decoded JSON value.
    pub value: Value,
    /// Raw line length in bytes.
    pub size: usize,
}

/// Why a stream yields no further frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// End of stream, including after a final frame without a trailing newline.
    Closed,
    /// A read failure, or a line that reached the frame limit without a newline.
    Read,
    /// A line that is not valid JSON.
    Decode,
}

impl End {
    /// Record this end on the adapter's stdout diagnostics.
    pub fn observe(self, diagnostics: &Diagnostics) {
        let (category, operation) = match self {
            Self::Closed => (Category::Closed, "read"),
            Self::Read => (Category::Protocol, "read"),
            Self::Decode => (Category::Protocol, "decode"),
        };
        diagnostics.observe(Diagnostic::new(category, "stdout"), operation);
    }
}

/// Reader of newline-delimited JSON frames of at most `limit` bytes each.
///
/// Blank lines are skipped. A final line without a newline is still delivered when it is shorter
/// than the limit, because a short read proves end of stream; a line that fills the limit without
/// a newline may have unread bytes behind it and fails instead of delivering a prefix.
#[derive(Debug)]
pub struct JsonLines<R> {
    output: BufReader<R>,
    limit: usize,
    ended: bool,
}

impl<R: AsyncRead + Unpin> JsonLines<R> {
    /// Read frames of at most `limit` bytes, newline included, from `output`.
    pub fn new(output: R, limit: usize) -> Self {
        Self {
            output: BufReader::new(output),
            limit,
            ended: false,
        }
    }

    /// Read the next non-blank frame.
    ///
    /// # Errors
    ///
    /// Returns why the stream ended; callers stop reading at the first error.
    pub async fn next(&mut self) -> Result<Frame, End> {
        if self.ended {
            return Err(End::Closed);
        }
        loop {
            let mut bytes = Vec::new();
            let read = (&mut self.output)
                .take(self.limit as u64)
                .read_until(b'\n', &mut bytes)
                .await;
            let terminated = bytes.last() == Some(&b'\n');
            let trailing = matches!(read, Ok(1..)) && bytes.len() < self.limit && !terminated;
            if !matches!(read, Ok(1..)) || (!terminated && !trailing) {
                return Err(if matches!(read, Ok(0)) {
                    End::Closed
                } else {
                    End::Read
                });
            }
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value = serde_json::from_slice(&bytes).map_err(|_| End::Decode)?;
            self.ended = trailing;
            return Ok(Frame {
                value,
                size: bytes.len(),
            });
        }
    }
}

#[cfg(test)]
mod tests;
