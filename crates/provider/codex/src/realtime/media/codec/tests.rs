use super::*;

#[test]
fn silence_does_not_produce_downstream_pcm() {
    let mut encoder = Encoder::new().unwrap();
    let mut decoder = Decoder::new().unwrap();

    for sequence in 0..200 {
        decoder.push(sequence, encoder.frame().unwrap()).unwrap();
        if sequence % 4 == 3 {
            assert!(
                decoder.flush().unwrap().is_none(),
                "silent Opus must not produce a downstream PCM chunk"
            );
        }
    }
}

#[test]
fn no_remote_packets_do_not_produce_pcm_on_repeated_flushes() {
    let mut decoder = Decoder::new().unwrap();

    for _ in 0..200 {
        assert!(decoder.flush().unwrap().is_none());
    }
}

#[test]
fn very_quiet_opus_does_not_produce_downstream_pcm() {
    for amplitude in [1, 8] {
        let mut encoder = Encoder::new().unwrap();
        encoder.push(&square_wave(amplitude, 4)).unwrap();
        let mut decoder = Decoder::new().unwrap();

        for sequence in 0..40 {
            decoder.push(sequence, encoder.frame().unwrap()).unwrap();
            if sequence % 4 == 3 {
                assert!(
                    decoder.flush().unwrap().is_none(),
                    "very quiet Opus must not produce downstream PCM: amplitude {amplitude}"
                );
            }
        }
    }
}

#[test]
fn reply_after_suppressed_silence_preserves_the_continuous_decoder_and_every_sample() {
    let mut encoder = Encoder::new().unwrap();
    let mut decoder = Decoder::new().unwrap();
    let mut reference = opus2::Decoder::new(RATE, Channels::Mono).unwrap();
    let mut sequence = 0;

    for amplitude in [128, -128] {
        for _ in 0..80 {
            let packet = encoder.frame().unwrap();
            decoder.push(sequence, packet.clone()).unwrap();
            reference
                .decode(&packet, &mut [0_i16; FRAME], false)
                .unwrap();
            sequence += 1;
            if sequence % 4 == 0 {
                decoder.flush().unwrap();
            }
        }
        assert!(decoder.flush().unwrap().is_none());
        encoder.push(&square_wave(amplitude, 4)).unwrap();
        let mut expected = Vec::new();
        for _ in 0..4 {
            let packet = encoder.frame().unwrap();
            decoder.push(sequence, packet.clone()).unwrap();
            let mut samples = [0_i16; FRAME];
            let count = reference.decode(&packet, &mut samples, false).unwrap();
            expected.extend(
                samples[..count]
                    .iter()
                    .flat_map(|sample| sample.to_le_bytes()),
            );
            sequence += 1;
        }
        let audio = decoder
            .flush()
            .unwrap()
            .expect("quiet reply should survive the gate");
        assert_eq!(audio.sample_rate, RATE);
        assert_eq!(audio.bytes.len(), 4 * FRAME * 2);
        assert_eq!(audio.bytes, expected);
    }
}

fn square_wave(amplitude: i16, frames: usize) -> Audio {
    Audio {
        bytes: (0..frames * FRAME)
            .flat_map(|index| {
                (if index % 40 < 20 {
                    amplitude
                } else {
                    -amplitude
                })
                .to_le_bytes()
            })
            .collect(),
        sample_rate: RATE,
    }
}

#[test]
fn interruption_discards_old_audio_without_resetting_the_next_reply_decoder() {
    let mut encoder = Encoder::new().unwrap();
    encoder
        .push(&Audio {
            bytes: (0..1920)
                .flat_map(|i| (if i % 40 < 20 { 12_000_i16 } else { -12_000_i16 }).to_le_bytes())
                .collect(),
            sample_rate: RATE,
        })
        .unwrap();
    let mut reference = Decoder::new().unwrap();
    let mut interrupted = Decoder::new().unwrap();
    let packets: Vec<_> = (0..4).map(|_| encoder.frame().unwrap()).collect();
    // The second packet remains in the reorder buffer until flush/discard resolves the gap.
    for index in [0, 2] {
        reference
            .push(index, packets[usize::from(index)].clone())
            .unwrap();
        interrupted
            .push(index, packets[usize::from(index)].clone())
            .unwrap();
    }
    reference.flush().unwrap();
    interrupted.discard().unwrap();
    assert!(interrupted.flush().unwrap().is_none());
    reference.push(3, packets[3].clone()).unwrap();
    interrupted.push(3, packets[3].clone()).unwrap();
    assert_eq!(
        interrupted.flush().unwrap().unwrap().bytes,
        reference.flush().unwrap().unwrap().bytes
    );
}

#[test]
fn opus_round_trip_preserves_rate_duration_and_signal_after_fragmented_capture() {
    let samples: Vec<u8> = (0..1600)
        .flat_map(|i| {
            let value = if i % 40 < 20 { 12_000_i16 } else { -12_000_i16 };
            value.to_le_bytes()
        })
        .collect();
    let mut encoder = Encoder::new().unwrap();
    for chunk in samples.chunks(46) {
        encoder
            .push(&Audio {
                bytes: chunk.to_vec(),
                sample_rate: 16_000,
            })
            .unwrap();
    }
    let mut decoder = Decoder::new().unwrap();
    for sequence in 0..5 {
        decoder.push(sequence, encoder.frame().unwrap()).unwrap();
    }
    let audio = decoder.flush().unwrap().unwrap();
    assert_eq!(audio.sample_rate, 24_000);
    assert_eq!(audio.bytes.len(), 4800);
    assert!(
        audio
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .any(|sample| i16::from_le_bytes(*sample).unsigned_abs() > 1000)
    );
    assert!(decoder.flush().unwrap().is_none());
}

#[test]
fn reordered_duplicate_and_wrapping_packets_do_not_duplicate_audio() {
    let mut encoder = Encoder::new().unwrap();
    encoder.push(&square_wave(12_000, 4)).unwrap();
    let mut decoder = Decoder::new().unwrap();
    for sequence in [65534, 0, 65535, 0, 1] {
        decoder.push(sequence, encoder.frame().unwrap()).unwrap();
    }
    assert_eq!(decoder.flush().unwrap().unwrap().bytes.len(), 4 * FRAME * 2);
}

#[test]
fn short_packet_loss_is_concealed_and_final_pending_audio_is_flushed() {
    let mut encoder = Encoder::new().unwrap();
    encoder.push(&square_wave(12_000, 3)).unwrap();
    let mut decoder = Decoder::new().unwrap();
    for sequence in [10, 12] {
        decoder.push(sequence, encoder.frame().unwrap()).unwrap();
    }
    assert_eq!(decoder.flush().unwrap().unwrap().bytes.len(), 3 * FRAME * 2);
}

#[test]
fn microphone_and_remote_packet_budgets_reject_invalid_or_unbounded_audio() {
    for audio in [
        Audio {
            bytes: vec![0],
            sample_rate: 16_000,
        },
        Audio {
            bytes: vec![0, 0],
            sample_rate: 0,
        },
        Audio {
            bytes: vec![0; MAX_SAMPLES * 2 + 2],
            sample_rate: RATE,
        },
    ] {
        assert!(Encoder::new().unwrap().push(&audio).is_err());
    }
    let mut encoder = Encoder::new().unwrap();
    encoder
        .push(&Audio {
            bytes: vec![0; 4],
            sample_rate: RATE,
        })
        .unwrap();
    assert!(
        encoder
            .push(&Audio {
                bytes: vec![0; 4],
                sample_rate: 16_000
            })
            .is_err()
    );
    assert!(Decoder::new().unwrap().push(1, vec![]).is_err());
    assert!(Decoder::new().unwrap().push(1, vec![0; 4097]).is_err());
}
