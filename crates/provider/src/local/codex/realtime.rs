use base64::{Engine, engine::general_purpose::STANDARD};
use model::realtime::{Audio, Endpoint, Event};
use serde_json::{Value, json};

use super::{AgentSessionError, CodexSession};

const MAX_AUDIO_BYTES: usize = 512 * 1024;

#[derive(Debug)]
pub(super) struct Realtime {
    endpoint: Endpoint,
    user_speaking: bool,
    resampler: Resampler,
}

impl CodexSession {
    pub(super) async fn open_realtime(
        &mut self,
        endpoint: Endpoint,
    ) -> Result<(), AgentSessionError> {
        if self.history
            || self.realtime.is_some()
            || self.active_turn.is_some()
            || endpoint.cancel.is_cancelled()
        {
            return Err(AgentSessionError::Rejected);
        }
        let transport = if let Some(sdp) = &endpoint.offer {
            if sdp.is_empty() || sdp.len() > 128 * 1024 {
                return Err(AgentSessionError::Rejected);
            }
            json!({"type":"webrtc","sdp":sdp})
        } else {
            json!({"type":"websocket"})
        };
        let mut params = json!({"threadId":self.id,"outputModality":"audio","transport":transport});
        if endpoint.offer.is_some() {
            // AVAS uses the v3 handshake; the native WebRTC default is still legacy v1.
            params["version"] = json!("v3");
        }
        self.transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?
            .request("thread/realtime/start", params)
            .await?;
        self.realtime = Some(Realtime {
            endpoint,
            user_speaking: false,
            resampler: Resampler::default(),
        });
        self.drive_realtime().await
    }

    pub(super) async fn drive_realtime(&mut self) -> Result<(), AgentSessionError> {
        let Some(realtime) = &mut self.realtime else {
            return Ok(());
        };
        if realtime.endpoint.cancel.is_cancelled() || realtime.endpoint.output.is_closed() {
            return self.stop_realtime().await;
        }
        // Keep native writes bounded so text events and approvals continue to drain.
        for _ in 0..8 {
            let Ok(audio) = realtime.endpoint.input.try_recv() else {
                break;
            };
            if realtime.endpoint.offer.is_some() || !valid_audio(&audio) {
                let _ = realtime.endpoint.output.try_send(Event::Failed);
                return self.stop_realtime().await;
            }
            let Some(samples) = realtime.resampler.convert(&audio) else {
                let _ = realtime.endpoint.output.try_send(Event::Failed);
                return self.stop_realtime().await;
            };
            if samples.is_empty() {
                continue;
            }
            let result = self
                .transport
                .as_mut()
                .ok_or(AgentSessionError::Failed)?
                .request(
                    "thread/realtime/appendAudio",
                    json!({"threadId":self.id,"audio":{"data":STANDARD.encode(&samples),
                    "sampleRate":24_000,"numChannels":1,
                    "samplesPerChannel":samples.len()/2,"itemId":null}}),
                )
                .await;
            if result.is_err() {
                let _ = realtime.endpoint.output.try_send(Event::Failed);
                return self.stop_realtime().await;
            }
        }
        Ok(())
    }

    async fn stop_realtime(&mut self) -> Result<(), AgentSessionError> {
        if self.realtime.take().is_some() {
            let transport = self.transport.as_mut().ok_or(AgentSessionError::Failed)?;
            let _ = transport
                .request("thread/realtime/stop", json!({"threadId":self.id}))
                .await;
            transport.wait_realtime_closed(&self.id).await?;
        }
        Ok(())
    }

    pub(super) fn observe_realtime(&mut self, method: &str, params: &Value) {
        let Some(realtime) = &mut self.realtime else {
            return;
        };
        let event = match method {
            "thread/realtime/item/started"
                if params["item"]["type"] == "transcriptSegment"
                    && params["item"]["role"] == "user" =>
            {
                realtime.user_speaking = true;
                Event::Interrupted
            }
            "thread/realtime/item/completed"
                if params["item"]["type"] == "transcriptSegment"
                    && params["item"]["role"] == "user" =>
            {
                realtime.user_speaking = false;
                let Some(text) = params["item"]["text"]
                    .as_str()
                    .filter(|text| text.len() <= 64 * 1024)
                else {
                    return;
                };
                Event::Transcript(text.to_owned())
            }
            "thread/realtime/sdp" => {
                let Some(sdp) = params["sdp"]
                    .as_str()
                    .filter(|sdp| !sdp.is_empty() && sdp.len() <= 128 * 1024)
                else {
                    realtime.endpoint.cancel.cancel();
                    return;
                };
                Event::Sdp(sdp.to_owned())
            }
            "thread/realtime/outputAudio/delta" => {
                decode_audio(&params["audio"]).map_or(Event::Failed, Event::Audio)
            }
            "thread/realtime/transcript/delta" if params["role"] == "user" => {
                if realtime.user_speaking || params["delta"].as_str().is_none_or(str::is_empty) {
                    return;
                }
                realtime.user_speaking = true;
                Event::Interrupted
            }
            "thread/realtime/transcript/done" if params["role"] == "user" => {
                realtime.user_speaking = false;
                let Some(text) = params["text"]
                    .as_str()
                    .filter(|text| text.len() <= 64 * 1024)
                else {
                    return;
                };
                Event::Transcript(text.to_owned())
            }
            "thread/realtime/error" => Event::Failed,
            "thread/realtime/closed" => {
                let _ = realtime.endpoint.output.try_send(Event::Closed);
                self.realtime = None;
                return;
            }
            _ => return,
        };
        let failed = matches!(event, Event::Failed);
        if realtime.endpoint.output.try_send(event).is_err() || failed {
            realtime.endpoint.cancel.cancel();
        }
    }
}

fn valid_audio(audio: &Audio) -> bool {
    matches!(
        audio.sample_rate,
        8_000 | 16_000 | 22_050 | 24_000 | 44_100 | 48_000
    ) && !audio.bytes.is_empty()
        && audio.bytes.len() <= MAX_AUDIO_BYTES
        && audio.bytes.len().is_multiple_of(2)
}

fn decode_audio(value: &Value) -> Option<Audio> {
    if value["numChannels"] != 1 {
        return None;
    }
    let data = value["data"].as_str()?;
    if data.len() > MAX_AUDIO_BYTES.div_ceil(3) * 4 {
        return None;
    }
    let audio = Audio {
        bytes: STANDARD.decode(data).ok()?,
        sample_rate: value["sampleRate"].as_u64()?.try_into().ok()?,
    };
    if let Some(samples) = value["samplesPerChannel"].as_u64()
        && samples != (audio.bytes.len() / 2) as u64
    {
        return None;
    }
    valid_audio(&audio).then_some(audio)
}

#[derive(Debug, Default)]
struct Resampler {
    rate: Option<u32>,
    previous: Option<i16>,
    phase: u32,
}

impl Resampler {
    fn convert(&mut self, audio: &Audio) -> Option<Vec<u8>> {
        if self.rate.is_some_and(|rate| rate != audio.sample_rate) {
            return None;
        }
        self.rate = Some(audio.sample_rate);
        let mut bytes =
            Vec::with_capacity(audio.bytes.len() * 24_000 / audio.sample_rate as usize + 4);
        for sample in audio.bytes.as_chunks::<2>().0 {
            let current = i16::from_le_bytes(*sample);
            if let Some(previous) = self.previous {
                while self.phase <= 24_000 {
                    let value = i64::from(previous)
                        + (i64::from(current) - i64::from(previous)) * i64::from(self.phase)
                            / 24_000;
                    bytes.extend_from_slice(&i16::try_from(value).ok()?.to_le_bytes());
                    self.phase += audio.sample_rate;
                }
                self.phase -= 24_000;
            } else {
                bytes.extend_from_slice(&current.to_le_bytes());
                self.phase = audio.sample_rate;
            }
            self.previous = Some(current);
        }
        Some(bytes)
    }
}

#[cfg(test)]
mod tests;
