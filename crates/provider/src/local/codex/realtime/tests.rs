use super::*;

#[test]
fn resampling_preserves_duration_and_phase_across_fragmented_input() {
    let samples: Vec<u8> = (0_i16..1600).flat_map(i16::to_le_bytes).collect();
    let whole = Resampler::default()
        .convert(&Audio {
            bytes: samples.clone(),
            sample_rate: 16000,
        })
        .unwrap();
    let mut resampler = Resampler::default();
    let fragmented: Vec<u8> = samples
        .chunks(14)
        .flat_map(|chunk| {
            resampler
                .convert(&Audio {
                    bytes: chunk.to_vec(),
                    sample_rate: 16000,
                })
                .unwrap()
        })
        .collect();
    assert_eq!(whole, fragmented);
    assert_eq!(whole.len(), 4798);
    assert!(
        resampler
            .convert(&Audio {
                bytes: vec![0, 0],
                sample_rate: 24000
            })
            .is_none()
    );
    let passthrough = Resampler::default()
        .convert(&Audio {
            bytes: samples.clone(),
            sample_rate: 24000,
        })
        .unwrap();
    assert_eq!(passthrough, samples);
}
