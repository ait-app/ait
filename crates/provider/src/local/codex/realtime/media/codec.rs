//! Bounded PCM/Opus conversion, with a short reorder window for upstream RTP.

use std::collections::VecDeque;

use model::realtime::Audio;
use opus2::{Application, Bitrate, Channels};

use super::super::{AgentSessionError, Resampler, valid_audio};

const RATE: u32 = 24_000;
const FRAME: usize = 480;
const MAX_SAMPLES: usize = 24_000;

/// Bounded continuous PCM capture encoder.
pub(super) struct Encoder {
    codec: opus2::Encoder,
    resampler: Resampler,
    samples: VecDeque<i16>,
}

impl Encoder {
    /// Construct a mono 24 kHz Opus codec, reporting codec allocation errors.
    pub(super) fn new() -> Result<Self, AgentSessionError> {
        let mut codec = opus2::Encoder::new(RATE, Channels::Mono, Application::Voip)
            .map_err(|_| AgentSessionError::Failed)?;
        codec
            .set_bitrate(Bitrate::Bits(32_000))
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(Self {
            codec,
            resampler: Resampler::default(),
            samples: VecDeque::new(),
        })
    }

    /// Validate and resample capture, rejecting incompatible rates and excess buffered audio.
    pub(super) fn push(&mut self, audio: &Audio) -> Result<(), AgentSessionError> {
        if !valid_audio(audio) {
            return Err(AgentSessionError::Failed);
        }
        let bytes = self
            .resampler
            .convert(audio)
            .ok_or(AgentSessionError::Failed)?;
        if self.samples.len() + bytes.len() / 2 > MAX_SAMPLES {
            return Err(AgentSessionError::Failed);
        }
        self.samples.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| i16::from_le_bytes(*sample)),
        );
        Ok(())
    }

    /// Encode the next 20 ms, padding missing microphone samples with silence.
    pub(super) fn frame(&mut self) -> Result<Vec<u8>, AgentSessionError> {
        let mut samples = [0; FRAME];
        for sample in &mut samples {
            *sample = self.samples.pop_front().unwrap_or(0);
        }
        self.codec
            .encode_vec(&samples, 1275)
            .map_err(|_| AgentSessionError::Failed)
    }
}

/// Bounded RTP reorder and PCM decoding state.
pub(super) struct Decoder {
    codec: opus2::Decoder,
    next: Option<u16>,
    pending: Vec<(u16, Vec<u8>)>,
    samples: Vec<u8>,
}

impl Decoder {
    /// Construct a mono 24 kHz Opus codec, reporting codec allocation errors.
    pub(super) fn new() -> Result<Self, AgentSessionError> {
        Ok(Self {
            codec: opus2::Decoder::new(RATE, Channels::Mono)
                .map_err(|_| AgentSessionError::Failed)?,
            next: None,
            pending: Vec::new(),
            samples: Vec::new(),
        })
    }

    /// Reorder one bounded Opus packet, ignoring late or duplicate sequence numbers.
    pub(super) fn push(&mut self, sequence: u16, packet: Vec<u8>) -> Result<(), AgentSessionError> {
        if packet.is_empty() || packet.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
        let next = *self.next.get_or_insert(sequence);
        if sequence.wrapping_sub(next) >= 32768
            || self.pending.iter().any(|(id, _)| *id == sequence)
        {
            return Ok(());
        }
        if self.pending.len() >= 16 {
            return Err(AgentSessionError::Failed);
        }
        self.pending.push((sequence, packet));
        self.drain(false)
    }

    fn drain(&mut self, flush: bool) -> Result<(), AgentSessionError> {
        while let Some(next) = self.next {
            let Some((index, distance)) = self
                .pending
                .iter()
                .enumerate()
                .map(|(index, (sequence, _))| (index, sequence.wrapping_sub(next)))
                .min_by_key(|(_, distance)| *distance)
            else {
                break;
            };
            if distance > 0 && !flush && self.pending.len() < 3 {
                break;
            }
            // Conceal short losses without constructing unbounded silence for large discontinuities.
            for _ in 0..distance.min(3) {
                self.decode(&[], FRAME)?;
            }
            let (sequence, packet) = self.pending.swap_remove(index);
            self.decode(&packet, 2880)?;
            self.next = Some(sequence.wrapping_add(1));
        }
        Ok(())
    }

    fn decode(&mut self, packet: &[u8], capacity: usize) -> Result<(), AgentSessionError> {
        let mut samples = [0_i16; 2880];
        let count = self
            .codec
            .decode(packet, &mut samples[..capacity], false)
            .map_err(|_| AgentSessionError::Failed)?;
        if self.samples.len() + count * 2 > MAX_SAMPLES * 2 {
            return Err(AgentSessionError::Failed);
        }
        self.samples.extend(
            samples[..count]
                .iter()
                .flat_map(|sample| sample.to_le_bytes()),
        );
        Ok(())
    }

    /// Finish pending loss concealment and take decoded PCM, if any.
    pub(super) fn flush(&mut self) -> Result<Option<Audio>, AgentSessionError> {
        self.drain(true)?;
        Ok((!self.samples.is_empty()).then(|| Audio {
            bytes: std::mem::take(&mut self.samples),
            sample_rate: RATE,
        }))
    }
}

#[cfg(test)]
mod tests;
