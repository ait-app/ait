//! Codex's upstream WebRTC peer. Neither SDP nor upstream addresses cross the provider port.

mod codec;

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use model::realtime::{Endpoint, Event};
use rtc::{
    interceptor::Registry,
    media::Sample,
    media_stream::MediaStreamTrack,
    peer_connection::configuration::interceptor_registry::register_default_interceptors,
    rtp_transceiver::rtp_sender::{
        RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
        RtpCodecKind,
    },
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use webrtc::{
    data_channel::{DataChannel, DataChannelEvent},
    media_stream::{
        track_local::static_sample::TrackLocalStaticSample,
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
        RTCIceGatheringState, RTCPeerConnectionState, RTCSessionDescription,
    },
};

use super::AgentSessionError;

/// Provider-owned media constructor; tests can replace the external transport boundary.
pub(in crate::local::codex) type Factory =
    fn(Endpoint) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>>;

/// A cancellable provider-owned peer with native negotiation and interruption controls.
#[derive(Debug)]
pub(in crate::local::codex) struct Media {
    pub(super) offer: String,
    answer: Option<oneshot::Sender<String>>,
    cancel: CancellationToken,
    interruptions: watch::Sender<()>,
}

impl Drop for Media {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Media {
    /// Discard buffered playback once on interruption, without gating subsequent reply audio.
    pub(super) fn interrupt(&self) {
        self.interruptions.send_replace(());
    }

    /// Apply the single native SDP answer, failing if startup was cancelled or already answered.
    pub(super) fn answer(&mut self, sdp: String) -> Result<(), AgentSessionError> {
        self.answer
            .take()
            .ok_or(AgentSessionError::Failed)?
            .send(sdp)
            .map_err(|_| AgentSessionError::Failed)
    }
}

/// Gather an upstream offer within the caller lifetime and bounded startup deadline.
pub(in crate::local::codex) fn open(
    endpoint: Endpoint,
) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>> {
    Box::pin(async move {
        let cancel = endpoint.cancel.clone();
        let result = tokio::select! {
            () = cancel.cancelled() => return Err(AgentSessionError::Failed),
            result = tokio::time::timeout(Duration::from_secs(10), prepare(endpoint)) => result,
        };
        result.map_err(|_| AgentSessionError::Failed)?
    })
}

#[derive(Clone)]
struct Handler {
    gathered: mpsc::Sender<()>,
    connected: mpsc::Sender<()>,
    tracks: mpsc::Sender<Arc<dyn TrackRemote>>,
    ended: CancellationToken,
}

// Match WebRTC's boxed callback contract, including its receiver/future lifetime bounds.
impl PeerConnectionEventHandler for Handler {
    fn on_ice_gathering_state_change<'a, 'future>(
        &'a self,
        state: RTCIceGatheringState,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'future>>
    where
        'a: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            if state == RTCIceGatheringState::Complete {
                let _ = self.gathered.try_send(());
            }
        })
    }

    fn on_connection_state_change<'a, 'future>(
        &'a self,
        state: RTCPeerConnectionState,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'future>>
    where
        'a: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            match state {
                RTCPeerConnectionState::Connected => {
                    let _ = self.connected.try_send(());
                }
                RTCPeerConnectionState::Failed => {
                    tracing::warn!(reason = "peer_failed", "Codex realtime peer ended");
                    self.ended.cancel();
                }
                RTCPeerConnectionState::Closed => {
                    tracing::info!(reason = "peer_closed", "Codex realtime peer ended");
                    self.ended.cancel();
                }
                _ => {}
            }
        })
    }

    fn on_track<'a, 'future>(
        &'a self,
        track: Arc<dyn TrackRemote>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'future>>
    where
        'a: 'future,
        Self: 'future,
    {
        Box::pin(async move {
            if self.tracks.try_send(track).is_err() {
                tracing::warn!(
                    reason = "remote_track_queue_unavailable",
                    "Codex realtime peer ended"
                );
                self.ended.cancel();
            }
        })
    }
}

async fn prepare(endpoint: Endpoint) -> Result<Media, AgentSessionError> {
    let (gathered, mut gathering) = mpsc::channel(1);
    let (connected, connection) = mpsc::channel(1);
    let (tracks, incoming) = mpsc::channel(1);
    let ended = CancellationToken::new();
    let codec = RTCRtpCodec {
        mime_type: "audio/opus".to_owned(),
        clock_rate: 48_000,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
        ..Default::default()
    };
    let peer = create_peer(
        Handler {
            gathered,
            connected,
            tracks,
            ended: ended.clone(),
        },
        codec.clone(),
        endpoint.cancel.clone(),
    )
    .await?;
    let (answer, answer_rx) = oneshot::channel();
    let (interruptions, interrupt) = watch::channel(());
    let mut media = Media {
        offer: String::new(),
        answer: Some(answer),
        cancel: endpoint.cancel.clone(),
        interruptions,
    };
    let (track, ssrc) = microphone(codec)?;
    peer.add_track(track.clone())
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    let data = peer
        .create_data_channel("oai-events", None)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    let offer = peer
        .create_offer(None)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    peer.set_local_description(offer)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    gathering.recv().await.ok_or(AgentSessionError::Failed)?;
    media.offer = peer
        .local_description()
        .await
        .ok_or(AgentSessionError::Failed)?
        .sdp;
    if media.offer.is_empty() || media.offer.len() > 128 * 1024 {
        return Err(AgentSessionError::Failed);
    }
    tokio::spawn(
        async move {
            let output = endpoint.output.clone();
            let cancel = endpoint.cancel.clone();
            let running = run(
                peer,
                answer_rx,
                connection,
                MediaIo {
                    endpoint,
                    track,
                    ssrc,
                    incoming,
                    interruptions: interrupt,
                },
            );
            let (reason, event) = tokio::select! {
                biased;
                () = cancel.cancelled() => ("caller_cancelled", None),
                () = ended.cancelled() => ("peer_ended", Some(Event::Failed)),
                reason = drain_data(data) => (reason, Some(Event::Closed)),
                result = running => {
                    if result.is_err() {
                        ("media_io_failed", Some(Event::Failed))
                    } else {
                        ("media_io_ended", None)
                    }
                }
            };
            if matches!(&event, Some(Event::Failed)) || reason == "data_channel_error" {
                tracing::warn!(reason, "Codex realtime media stopped");
            } else {
                tracing::info!(reason, "Codex realtime media stopped");
            }
            if let Some(event) = event {
                let _ = output.try_send(event);
            }
            cancel.cancel();
        }
        .in_current_span(),
    );
    Ok(media)
}

async fn create_peer(
    handler: Handler,
    codec: RTCRtpCodec,
    cancel: CancellationToken,
) -> Result<Arc<dyn PeerConnection>, AgentSessionError> {
    let mut engine = MediaEngine::default();
    engine
        .register_codec(
            RTCRtpCodecParameters {
                rtp_codec: codec.clone(),
                payload_type: 111,
            },
            RtpCodecKind::Audio,
        )
        .map_err(|_| AgentSessionError::Failed)?;
    let registry = register_default_interceptors(Registry::new(), &mut engine)
        .map_err(|_| AgentSessionError::Failed)?;
    let peer = Box::pin(
        PeerConnectionBuilder::new()
            .with_media_engine(engine)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(handler))
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build(),
    )
    .await
    .map_err(|_| AgentSessionError::Failed)?;
    let peer: Arc<dyn PeerConnection> = Arc::new(peer);
    // This task also closes sockets if startup is abandoned before the actor is spawned.
    let lifetime = cancel;
    let closing = peer.clone();
    tokio::spawn(async move {
        lifetime.cancelled().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), closing.close()).await;
    });
    Ok(peer)
}

fn microphone(codec: RTCRtpCodec) -> Result<(Arc<TrackLocalStaticSample>, u32), AgentSessionError> {
    let ssrc = u32::from_le_bytes(
        uuid::Uuid::new_v4().as_bytes()[..4]
            .try_into()
            .expect("four UUID bytes"),
    );
    let track = Arc::new(
        TrackLocalStaticSample::new(
            std::time::Instant::now(),
            MediaStreamTrack::new(
                "ait-voice".to_owned(),
                "microphone".to_owned(),
                "microphone".to_owned(),
                RtpCodecKind::Audio,
                vec![RTCRtpEncodingParameters {
                    rtp_coding_parameters: RTCRtpCodingParameters {
                        ssrc: Some(ssrc),
                        ..Default::default()
                    },
                    codec,
                    ..Default::default()
                }],
            ),
        )
        .map_err(|_| AgentSessionError::Failed)?,
    );
    Ok((track, ssrc))
}

struct MediaIo {
    endpoint: Endpoint,
    track: Arc<TrackLocalStaticSample>,
    ssrc: u32,
    incoming: mpsc::Receiver<Arc<dyn TrackRemote>>,
    interruptions: watch::Receiver<()>,
}

async fn run(
    peer: Arc<dyn PeerConnection>,
    answer: oneshot::Receiver<String>,
    mut connected: mpsc::Receiver<()>,
    io: MediaIo,
) -> Result<(), AgentSessionError> {
    let sdp = answer.await.map_err(|_| AgentSessionError::Failed)?;
    let answer = RTCSessionDescription::answer(sdp).map_err(|_| AgentSessionError::Failed)?;
    peer.set_remote_description(answer)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    tokio::time::timeout(Duration::from_secs(20), connected.recv())
        .await
        .map_err(|_| AgentSessionError::Failed)?
        .ok_or(AgentSessionError::Failed)?;
    let MediaIo {
        endpoint,
        track,
        ssrc,
        incoming,
        interruptions,
    } = io;
    let Endpoint {
        ready,
        input,
        output,
        ..
    } = endpoint;
    ready.send(()).map_err(|()| AgentSessionError::Failed)?;
    let (stage, result) = tokio::select! {
        result = send_audio(input, track, ssrc) => ("microphone", result),
        result = receive_audio(incoming, output, interruptions) => ("playback", result),
    };
    if result.is_err() {
        tracing::warn!(stage, "Codex realtime audio I/O stopped");
    }
    result
}

async fn send_audio(
    mut input: mpsc::Receiver<model::realtime::Audio>,
    track: Arc<TrackLocalStaticSample>,
    ssrc: u32,
) -> Result<(), AgentSessionError> {
    let mut encoder = codec::Encoder::new()?;
    let mut clock = tokio::time::interval(Duration::from_millis(20));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            audio = input.recv() => encoder.push(&audio.ok_or(AgentSessionError::Failed)?)?,
            _ = clock.tick() => {
                let sample = Sample { data: encoder.frame()?.into(), duration: Duration::from_millis(20), ..Sample::new(std::time::Instant::now()) };
                track.write_sample(ssrc, 111, &sample, &[]).await.map_err(|_| AgentSessionError::Failed)?;
            }
        }
    }
}

async fn receive_audio(
    mut tracks: mpsc::Receiver<Arc<dyn TrackRemote>>,
    output: mpsc::Sender<Event>,
    mut interruptions: watch::Receiver<()>,
) -> Result<(), AgentSessionError> {
    let track = tracks.recv().await.ok_or(AgentSessionError::Failed)?;
    if track.kind().await != RtpCodecKind::Audio {
        return Err(AgentSessionError::Failed);
    }
    let mut decoder = codec::Decoder::new()?;
    let mut clock = tokio::time::interval(Duration::from_millis(80));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            changed = interruptions.changed() => {
                changed.map_err(|_| AgentSessionError::Failed)?;
                decoder.discard()?;
            }
            event = track.poll() => match event {
                Some(TrackRemoteEvent::OnRtpPacket(packet)) => {
                    decoder.push(packet.header.sequence_number, packet.payload.to_vec())?;
                }
                Some(TrackRemoteEvent::OnError | TrackRemoteEvent::OnEnding | TrackRemoteEvent::OnEnded) | None => return Err(AgentSessionError::Failed),
                Some(_) => {}
            },
            _ = clock.tick() => {
                if let Some(audio) = decoder.flush()? {
                    output.try_send(Event::Audio(audio)).map_err(|error| {
                        tracing::warn!(queue_full = matches!(error, mpsc::error::TrySendError::Full(_)),
                            "Codex realtime audio delivery failed at the downstream playback queue");
                        AgentSessionError::Failed
                    })?;
                }
            }
        }
    }
}

async fn drain_data(data: Arc<dyn DataChannel>) -> &'static str {
    // Native app-server owns transcripts, tools and approvals; only observe transport closure here.
    while let Some(event) = data.poll().await {
        match event {
            DataChannelEvent::OnClose => return "data_channel_closed",
            DataChannelEvent::OnClosing => return "data_channel_closing",
            DataChannelEvent::OnError => return "data_channel_error",
            _ => {}
        }
    }
    "data_channel_stream_ended"
}

#[cfg(test)]
pub(in crate::local::codex) mod tests;
