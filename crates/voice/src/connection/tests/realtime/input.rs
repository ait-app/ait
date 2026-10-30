use super::*;

#[tokio::test]
async fn quiet_native_audio_does_not_enter_the_pcm_channel() {
    let mut fixture = Fixture::new();
    let mut endpoint = native(&mut fixture, false);
    assert_eq!(fixture.mode(true).await["accepted"], true);

    for sample in [0_i16, 1, -1, 8, -8, 32, -32] {
        let bytes = sample.to_le_bytes().repeat(1600);
        fixture.event(
            "voice.audio.chunk",
            json!({"audio":STANDARD.encode(bytes),"format":"pcm;rate=16000","isLast":false}),
        );
        assert!(
            matches!(
                endpoint.input.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ),
            "quiet input reached the PCM channel: sample {sample}"
        );
    }

    for sample in [33_i16, -33, i16::MIN, i16::MAX] {
        let mut bytes = vec![0; 3200];
        bytes[3198..].copy_from_slice(&sample.to_le_bytes());
        fixture.event(
            "voice.audio.chunk",
            json!({"audio":STANDARD.encode(&bytes),"format":"pcm;rate=16000","isLast":false}),
        );
        let audio = endpoint
            .input
            .try_recv()
            .expect("speech should enter the PCM channel");
        assert_eq!(audio.bytes, bytes);
        assert_eq!(audio.sample_rate, 16000);
    }
    assert!(!endpoint.cancel.is_cancelled());
    assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sustained_quiet_input_does_not_fill_the_channel_or_cancel_the_session() {
    let mut fixture = Fixture::new();
    let mut endpoint = native(&mut fixture, false);
    assert_eq!(fixture.mode(true).await["accepted"], true);

    for _ in 0..128 {
        fixture.event(
            "voice.audio.chunk",
            json!({"audio":STANDARD.encode([32, 0, 224, 255]),"format":"pcm;rate=16000","isLast":false}),
        );
    }
    assert!(
        !endpoint.cancel.is_cancelled(),
        "quiet input exhausted the PCM channel"
    );
    assert!(matches!(
        endpoint.input.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.event(
        "voice.audio.chunk",
        json!({"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}),
    );
    assert_eq!(endpoint.input.try_recv().unwrap().bytes, [0, 8, 0, 8]);
    assert!(
        !fixture
            .events
            .iter()
            .any(|event| event["method"] == "voice.error")
    );
}
