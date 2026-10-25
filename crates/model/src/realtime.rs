//! Bounded, connection-owned audio transport between speech and a native Agent provider.

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Mono signed PCM16 little-endian audio. Adapters validate rate and byte budgets.
#[derive(Debug)]
pub struct Audio {
    /// Encoded sample bytes, without a container header.
    pub bytes: Vec<u8>,
    /// Samples per second.
    pub sample_rate: u32,
}

/// Provider-normalized realtime output, independent of native protocol names.
#[derive(Debug)]
pub enum Event {
    /// Incremental assistant audio.
    Audio(Audio),
    /// A completed user transcript; this is display-only, never a new text turn.
    Transcript(String),
    /// Native interruption; discard queued assistant playback.
    Interrupted,
    /// Native session ended normally.
    Closed,
    /// Native session failed; private upstream diagnostics are not transported.
    Failed,
}

/// Speech-side handle. Dropping it always cancels native recording and delivery.
#[derive(Debug)]
pub struct Session {
    /// Completes once the provider's upstream audio transport is ready.
    pub ready: oneshot::Receiver<()>,
    /// Bounded live microphone input.
    pub input: mpsc::Sender<Audio>,
    /// Bounded native output.
    pub output: mpsc::Receiver<Event>,
    cancel: CancellationToken,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Provider-side handle, retained by the native session owner.
#[derive(Debug)]
pub struct Endpoint {
    /// Signal readiness only after the provider's upstream media transport connects.
    pub ready: oneshot::Sender<()>,
    /// Input from the owning physical connection.
    pub input: mpsc::Receiver<Audio>,
    /// Output delivered only to that connection.
    pub output: mpsc::Sender<Event>,
    /// Cancelled when the client disconnects, stops, or abandons startup.
    pub cancel: CancellationToken,
}

/// Create bounded audio queues under the caller's lifetime token.
#[must_use]
pub fn channel(cancel: &CancellationToken) -> (Session, Endpoint) {
    let cancel = cancel.child_token();
    let (input, input_receiver) = mpsc::channel(16);
    let (output, output_receiver) = mpsc::channel(32);
    let (ready, readiness) = oneshot::channel();
    (
        Session {
            ready: readiness,
            input,
            output: output_receiver,
            cancel: cancel.clone(),
        },
        Endpoint {
            ready,
            input: input_receiver,
            output,
            cancel,
        },
    )
}
