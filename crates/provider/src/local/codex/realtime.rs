pub(super) mod media;

use model::realtime::{Audio, Endpoint, Event};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{AgentSessionError, CodexSession};

const MAX_AUDIO_BYTES: usize = 512 * 1024;

#[derive(Debug)]
pub(super) struct Realtime {
    media: media::Media,
    output: mpsc::Sender<Event>,
    cancel: CancellationToken,
    user_speaking: bool,
    session_id: Option<String>,
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
        let output = endpoint.output.clone();
        let cancel = endpoint.cancel.clone();
        let media = (self.client.realtime_media)(endpoint).await?;
        let params = json!({"threadId":self.id,"outputModality":"audio","version":"v3",
            "transport":{"type":"webrtc","sdp":media.offer}});
        self.transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?
            .request("thread/realtime/start", params)
            .await?;
        self.realtime = Some(Realtime {
            media,
            output,
            cancel,
            user_speaking: false,
            session_id: None,
        });
        self.drive_realtime().await
    }

    pub(super) async fn drive_realtime(&mut self) -> Result<(), AgentSessionError> {
        self.resolve_voice_items().await?;
        let Some(realtime) = &mut self.realtime else {
            return Ok(());
        };
        if realtime.cancel.is_cancelled() || realtime.output.is_closed() {
            return self.stop_realtime().await;
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

    pub(super) fn observe_realtime(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<(), AgentSessionError> {
        if let Some(event) = self.voice_timeline.observe(method, params)? {
            self.stream.events.push_back(event);
        }
        let Some(realtime) = &mut self.realtime else {
            return Ok(());
        };
        if let Some(session) = params["item"]["realtimeSessionId"].as_str()
            && realtime.session_id.as_deref() != Some(session)
        {
            return Ok(());
        }
        let event = match method {
            "thread/realtime/started" => {
                realtime.session_id = params["realtimeSessionId"].as_str().map(str::to_owned);
                return Ok(());
            }
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
                    return Ok(());
                };
                Event::Transcript(text.to_owned())
            }
            "thread/realtime/sdp" => {
                let Some(sdp) = params["sdp"]
                    .as_str()
                    .filter(|sdp| !sdp.is_empty() && sdp.len() <= 128 * 1024)
                else {
                    realtime.cancel.cancel();
                    return Ok(());
                };
                if realtime.media.answer(sdp.to_owned()).is_err() {
                    realtime.cancel.cancel();
                }
                return Ok(());
            }
            "thread/realtime/transcript/delta" if params["role"] == "user" => {
                if realtime.user_speaking || params["delta"].as_str().is_none_or(str::is_empty) {
                    return Ok(());
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
                    return Ok(());
                };
                Event::Transcript(text.to_owned())
            }
            "thread/realtime/error" => Event::Failed,
            "thread/realtime/closed" => {
                let _ = realtime.output.try_send(Event::Closed);
                self.realtime = None;
                return Ok(());
            }
            // Audio arrives through the provider-owned peer, never through outputAudio/delta.
            _ => return Ok(()),
        };
        if matches!(event, Event::Interrupted | Event::Transcript(_)) {
            realtime.media.speaking(realtime.user_speaking);
        }
        let failed = matches!(event, Event::Failed);
        if realtime.output.try_send(event).is_err() || failed {
            realtime.cancel.cancel();
        }
        Ok(())
    }

    async fn resolve_voice_items(&mut self) -> Result<(), AgentSessionError> {
        // Promotions reference ordinary persisted items; reuse their native identities/renderers.
        for _ in 0..4 {
            let Some((turn, id)) = self.voice_timeline.promoted.pop_first() else {
                break;
            };
            if self
                .voice_timeline
                .ordinary
                .contains(&(turn.clone(), id.clone()))
            {
                continue;
            }
            let response = self
                .transport
                .as_mut()
                .ok_or(AgentSessionError::Failed)?
                .request(
                    "thread/items/read",
                    json!({"threadId":self.id,"turnId":turn,"itemIds":[id]}),
                )
                .await?;
            let entries = response["data"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?;
            let item = entries
                .iter()
                .find(|entry| entry["turnId"] == turn && entry["item"]["id"] == id)
                .ok_or(AgentSessionError::Failed)?;
            self.voice_timeline.ordinary(&turn, &item["item"])?;
            let entries = super::discovery::timeline_items(
                &item["item"],
                &turn,
                &super::discovery::timestamp(),
                &self.client.images,
            )?;
            // Hydration is display-only: do not replay approvals or mutate active-turn state.
            self.stream.events.extend(
                entries
                    .into_iter()
                    .map(crate::ports::agent_session::AgentTurnEvent::Timeline),
            );
        }
        Ok(())
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
