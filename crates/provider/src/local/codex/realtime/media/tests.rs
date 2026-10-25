use super::*;

/// Offline media boundary for native app-server lifecycle tests.
pub(in crate::local::codex) fn fixture(
    endpoint: Endpoint,
) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>> {
    Box::pin(async move {
        let (answer, answer_rx) = oneshot::channel();
        let media = Media {
            offer: "fixture-offer".to_owned(),
            answer: Some(answer),
            cancel: endpoint.cancel.clone(),
            speaking: watch::channel(false).0,
        };
        tokio::spawn(async move {
            let Endpoint {
                ready,
                mut input,
                output,
                cancel,
            } = endpoint;
            tokio::select! {
                () = cancel.cancelled() => {}
                result = async {
                    let Ok(answer) = answer_rx.await else { return; };
                    assert_eq!(answer, "fixture-answer");
                    let _ = ready.send(());
                    while let Some(audio) = input.recv().await {
                        let bytes = super::super::Resampler::default().convert(&audio).unwrap();
                        for event in [Event::Interrupted, Event::Transcript("hello".to_owned()),
                            Event::Audio(model::realtime::Audio { bytes, sample_rate: 24_000 })] {
                            if output.send(event).await.is_err() { return; }
                        }
                    }
                } => result,
            }
        });
        Ok(media)
    })
}

#[tokio::test]
async fn invalid_answer_and_abandoned_start_close_the_provider_peer() {
    let cancel = CancellationToken::new();
    let (mut session, endpoint) = model::realtime::channel(&cancel);
    let mut media = open(endpoint).await.unwrap();
    assert!(media.offer.contains("opus/48000/2"));
    media.answer("invalid answer".to_owned()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut session.ready)
            .await
            .unwrap()
            .is_err()
    );
    assert!(matches!(session.output.recv().await, Some(Event::Failed)));
    assert!(media.cancel.is_cancelled());
    let (session, endpoint) = model::realtime::channel(&cancel);
    let media = open(endpoint).await.unwrap();
    drop(session);
    assert!(media.cancel.is_cancelled());
}

#[tokio::test]
async fn provider_peer_relays_pcm_in_both_directions_and_closes_on_session_drop() {
    tokio::time::timeout(Duration::from_secs(15), duplex())
        .await
        .unwrap();
}

async fn duplex() {
    use model::realtime::Audio;

    let (mut session, endpoint) = model::realtime::channel(&CancellationToken::new());
    let mut media = open(endpoint).await.unwrap();
    let (peer, track, mut incoming) = answer_peer(&mut media).await;
    (&mut session.ready).await.unwrap();

    let signal: Vec<u8> = (0..4800)
        .flat_map(|i| (if i % 40 < 20 { 12_000_i16 } else { -12_000_i16 }).to_le_bytes())
        .collect();
    session
        .input
        .send(Audio {
            bytes: signal.clone(),
            sample_rate: 24_000,
        })
        .await
        .unwrap();
    let remote = incoming.recv().await.unwrap();
    let mut decoder = codec::Decoder::new().unwrap();
    loop {
        if let Some(TrackRemoteEvent::OnRtpPacket(packet)) = remote.poll().await {
            decoder
                .push(packet.header.sequence_number, packet.payload.to_vec())
                .unwrap();
            if let Some(audio) = decoder.flush().unwrap()
                && audible(&audio)
            {
                break;
            }
        }
    }
    let mut encoder = codec::Encoder::new().unwrap();
    encoder
        .push(&Audio {
            bytes: signal,
            sample_rate: 24_000,
        })
        .unwrap();
    speak(&track, &mut encoder).await;
    loop {
        match session.output.recv().await.unwrap() {
            Event::Audio(audio) => {
                assert_eq!(audio.sample_rate, 24_000);
                if audible(&audio) {
                    break;
                }
            }
            other => panic!("unexpected media event: {other:?}"),
        }
    }
    drop(session);
    assert!(media.cancel.is_cancelled());
    peer.close().await.unwrap();
}

fn audible(audio: &model::realtime::Audio) -> bool {
    audio
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .any(|sample| i16::from_le_bytes(*sample).unsigned_abs() > 1000)
}

async fn answer_peer(
    media: &mut Media,
) -> (
    Arc<dyn PeerConnection>,
    Arc<TrackLocalStaticSample>,
    mpsc::Receiver<Arc<dyn TrackRemote>>,
) {
    let (gathered, mut gathering) = mpsc::channel(1);
    let (connected, _connection) = mpsc::channel(1);
    let (tracks, incoming) = mpsc::channel(1);
    let codec = RTCRtpCodec {
        mime_type: "audio/opus".to_owned(),
        clock_rate: 48_000,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
        ..Default::default()
    };
    let mut engine = MediaEngine::default();
    engine
        .register_codec(
            RTCRtpCodecParameters {
                rtp_codec: codec.clone(),
                payload_type: 111,
            },
            RtpCodecKind::Audio,
        )
        .unwrap();
    let peer = Box::pin(
        PeerConnectionBuilder::new()
            .with_media_engine(engine)
            .with_handler(Arc::new(Handler {
                gathered,
                connected,
                tracks,
                ended: CancellationToken::new(),
            }))
            .with_udp_addrs(vec!["127.0.0.1:0"])
            .build(),
    )
    .await
    .unwrap();
    let ssrc = 42;
    let track = Arc::new(
        TrackLocalStaticSample::new(
            std::time::Instant::now(),
            MediaStreamTrack::new(
                "remote".to_owned(),
                "speaker".to_owned(),
                "speaker".to_owned(),
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
        .unwrap(),
    );
    peer.add_track(track.clone()).await.unwrap();
    peer.set_remote_description(RTCSessionDescription::offer(media.offer.clone()).unwrap())
        .await
        .unwrap();
    let answer = peer.create_answer(None).await.unwrap();
    peer.set_local_description(answer).await.unwrap();
    gathering.recv().await.unwrap();
    media
        .answer(peer.local_description().await.unwrap().sdp)
        .unwrap();
    (Arc::new(peer), track, incoming)
}

async fn speak(track: &TrackLocalStaticSample, encoder: &mut codec::Encoder) {
    for _ in 0..10 {
        track
            .write_sample(
                42,
                111,
                &Sample {
                    data: encoder.frame().unwrap().into(),
                    duration: Duration::from_millis(20),
                    ..Sample::new(std::time::Instant::now())
                },
                &[],
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

mod native;

#[tokio::test]
async fn native_interruption_discards_media_until_the_user_finishes_speaking() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let (mut session, endpoint) = model::realtime::channel(&CancellationToken::new());
        let mut media = open(endpoint).await.unwrap();
        let (peer, track, _incoming) = answer_peer(&mut media).await;
        (&mut session.ready).await.unwrap();
        let mut encoder = codec::Encoder::new().unwrap();
        media.speaking(true);
        speak(&track, &mut encoder).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(150), session.output.recv())
                .await
                .is_err()
        );
        media.speaking(false);
        speak(&track, &mut encoder).await;
        assert!(matches!(session.output.recv().await, Some(Event::Audio(_))));
        drop(session);
        peer.close().await.unwrap();
    })
    .await
    .unwrap();
}
