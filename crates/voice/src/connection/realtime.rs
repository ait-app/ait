use model::{
    outbound::{Outbound, QueueError},
    realtime::Event,
};
use serde_json::json;

use super::{emit, voice::Voice, voice_error};
use crate::{
    Error,
    audio::{Audio, Format, decode_chunk},
    protocol,
};

impl Voice {
    pub(super) fn native_chunk(&mut self, chunk: &protocol::VoiceChunk) -> Result<(), Error> {
        let Format::Pcm(sample_rate) = Format::parse(&chunk.format)? else {
            return Err(Error::Invalid);
        };
        if chunk.audio.is_empty() && chunk.is_last {
            return Ok(());
        }
        let bytes = decode_chunk(&chunk.audio)?;
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::Invalid);
        }
        let native = self.native.as_ref().ok_or(Error::Realtime)?;
        if let Err(error) = native
            .input
            .try_send(model::realtime::Audio { bytes, sample_rate })
        {
            self.log_release(match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => "native_input_queue_full",
                tokio::sync::mpsc::error::TrySendError::Closed(_) => "native_input_closed",
            });
            self.abort();
            self.target = None;
            return Err(Error::Realtime);
        }
        Ok(())
    }

    pub(super) fn poll_native(&mut self, outbound: &Outbound) -> Result<(), QueueError> {
        let mut overflowed = false;
        for _ in 0..32 {
            let Some(native) = &mut self.native else {
                break;
            };
            let event = match native.output.try_recv() {
                Ok(event) => event,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    return self.close_native(outbound, true, "native_output_closed");
                }
            };
            match event {
                Event::Audio(audio) => {
                    if self.playback.len() >= 32 {
                        // Still read this bounded batch: a following interruption can retire
                        // the old audio. Never let playback backpressure hide lifecycle events.
                        overflowed = true;
                    } else {
                        self.output(Audio {
                            bytes: audio.bytes,
                            format: Format::Pcm(audio.sample_rate),
                        });
                    }
                }
                Event::Transcript(text) => {
                    emit(outbound, "voice.input.state", json!({"isSpeaking":false}))?;
                    emit(
                        outbound,
                        "voice.transcription.result",
                        json!({"text":text,"requestId":self.generation}),
                    )?;
                }
                Event::Interrupted => {
                    overflowed = false;
                    self.playback.clear();
                    self.acknowledgments.clear();
                    emit(outbound, "voice.input.state", json!({"isSpeaking":true}))?;
                }
                Event::Closed => {
                    return self.close_native(outbound, false, "native_closed");
                }
                Event::Failed => {
                    return self.close_native(outbound, true, "native_failed");
                }
            }
        }
        if overflowed {
            return self.close_native(outbound, true, "playback_queue_full");
        }
        Ok(())
    }

    fn close_native(
        &mut self,
        outbound: &Outbound,
        failed: bool,
        reason: &'static str,
    ) -> Result<(), QueueError> {
        self.log_release(reason);
        self.abort();
        self.target = None;
        if failed {
            voice_error(outbound, Error::Realtime)?;
        }
        emit(outbound, "voice.session.closed", json!({}))
    }
}

impl super::Connection {
    pub(super) async fn prepare_native(
        &self,
        agents: &dyn crate::ports::Agents,
        target: &str,
    ) -> Result<Option<model::realtime::Session>, Error> {
        let mut native = agents.realtime(target, self.cancel.clone()).await?;
        if let Some(session) = native.as_mut() {
            tokio::time::timeout(std::time::Duration::from_secs(25), &mut session.ready)
                .await
                .map_err(|_| Error::Realtime)?
                .map_err(|_| Error::Realtime)?;
        }
        Ok(native)
    }
}
