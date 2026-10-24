use super::*;

#[test]
fn native_pcm_decoding_checks_metadata_and_bounds() {
    let value = json!({"data":"AAgACA==","sampleRate":24000,"numChannels":1,"samplesPerChannel":2});
    let audio = decode_audio(&value).unwrap();
    assert_eq!(audio.bytes, [0, 8, 0, 8]);
    assert_eq!(audio.sample_rate, 24000);
    for patch in [
        json!({"numChannels":2}),
        json!({"sampleRate":0}),
        json!({"data":"AA=="}),
        json!({"samplesPerChannel":3}),
    ] {
        let mut invalid = value.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(decode_audio(&invalid).is_none());
    }
}

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
