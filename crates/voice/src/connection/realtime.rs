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
        if native
            .input
            .try_send(model::realtime::Audio { bytes, sample_rate })
            .is_err()
        {
            self.abort();
            self.target = None;
            return Err(Error::Realtime);
        }
        Ok(())
    }

    pub(super) fn poll_native(&mut self, outbound: &Outbound) -> Result<(), QueueError> {
        for _ in 0..32 {
            if self.playback.len() >= 32 {
                break;
            }
            let Some(native) = &mut self.native else {
                break;
            };
            let event = match native.output.try_recv() {
                Ok(event) => event,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => Event::Failed,
            };
            match event {
                Event::Audio(audio) => self.output(Audio {
                    bytes: audio.bytes,
                    format: Format::Pcm(audio.sample_rate),
                }),
                Event::Transcript(text) => {
                    emit(outbound, "voice.input.state", json!({"isSpeaking":false}))?;
                    emit(
                        outbound,
                        "voice.transcription.result",
                        json!({"text":text,"requestId":self.generation}),
                    )?;
                }
                Event::Interrupted => {
                    self.playback.clear();
                    self.acknowledgments.clear();
                    emit(outbound, "voice.input.state", json!({"isSpeaking":true}))?;
                }
                Event::Closed | Event::Failed => {
                    self.abort();
                    self.target = None;
                    if matches!(event, Event::Failed) {
                        voice_error(outbound, Error::Realtime)?;
                    }
                    emit(outbound, "voice.session.closed", json!({}))?;
                    break;
                }
            }
        }
        Ok(())
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
