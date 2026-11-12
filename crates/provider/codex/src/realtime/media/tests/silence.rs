use super::*;

#[tokio::test]
async fn silent_and_very_quiet_remote_media_do_not_enter_the_downstream_queue() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (mut session, endpoint) = model::realtime::channel(&CancellationToken::new());
        let mut media = open(endpoint).await.unwrap();
        let (peer, track, _incoming) = answer_peer(&mut media).await;
        (&mut session.ready).await.unwrap();

        let mut encoder = codec::Encoder::new().unwrap();
        for amplitude in [0_i16, 8] {
            encoder
                .push(&model::realtime::Audio {
                    bytes: (0..4800)
                        .flat_map(|index| {
                            (if index % 40 < 20 {
                                amplitude
                            } else {
                                -amplitude
                            })
                            .to_le_bytes()
                        })
                        .collect(),
                    sample_rate: 24_000,
                })
                .unwrap();
            speak(&track, &mut encoder).await;
            let received =
                tokio::time::timeout(Duration::from_millis(160), session.output.recv()).await;
            assert!(
                received.is_err(),
                "silent remote media reached the downstream queue"
            );
            assert!(!media.cancel.is_cancelled());
        }

        encoder
            .push(&model::realtime::Audio {
                bytes: (0..4800)
                    .flat_map(|index| {
                        (if index % 40 < 20 { 128_i16 } else { -128_i16 }).to_le_bytes()
                    })
                    .collect(),
                sample_rate: 24_000,
            })
            .unwrap();
        speak(&track, &mut encoder).await;
        let Some(Event::Audio(audio)) = session.output.recv().await else {
            panic!("expected a quiet reply after suppressed silence")
        };
        assert_eq!(audio.sample_rate, 24_000);
        assert!(
            audio
                .bytes
                .as_chunks::<2>()
                .0
                .iter()
                .any(|sample| { i16::from_le_bytes(*sample).unsigned_abs() > 32 })
        );

        drop(session);
        assert!(media.cancel.is_cancelled());
        peer.close().await.unwrap();
    })
    .await
    .unwrap();
}
