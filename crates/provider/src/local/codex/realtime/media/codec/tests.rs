use super::*;

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
    let mut decoder = Decoder::new().unwrap();
    for sequence in [65534, 0, 65535, 0, 1] {
        decoder.push(sequence, encoder.frame().unwrap()).unwrap();
    }
    assert_eq!(decoder.flush().unwrap().unwrap().bytes.len(), 4 * FRAME * 2);
}

#[test]
fn short_packet_loss_is_concealed_and_final_pending_audio_is_flushed() {
    let mut encoder = Encoder::new().unwrap();
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
