use super::*;

#[test]
fn empty_silent_and_very_quiet_pcm_are_suppressed() {
    assert!(is_quiet_pcm(&[]));
    for sample in [-32_i16, -8, -1, 0, 1, 8, 32] {
        assert!(is_quiet_pcm(&sample.to_le_bytes().repeat(1600)));
    }
}

#[test]
fn any_sample_above_the_peak_keeps_the_complete_chunk() {
    for sample in [i16::MIN, -33, 33, i16::MAX] {
        let mut bytes = vec![0; 3200];
        bytes[3198..].copy_from_slice(&sample.to_le_bytes());
        assert!(!is_quiet_pcm(&bytes));
    }
}

#[test]
fn malformed_pcm_is_not_hidden_by_the_silence_gate() {
    for bytes in [&[0][..], &[0, 0, 0][..]] {
        assert!(!is_quiet_pcm(bytes));
    }
}
