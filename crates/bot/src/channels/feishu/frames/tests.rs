use super::*;
use prost::Message;

fn fragment(sequence: usize, payload: &[u8]) -> Frame {
    Frame {
        seq_id: 7,
        log_id: 9,
        service: 12,
        method: 1,
        headers: vec![
            Header {
                key: "sum".into(),
                value: "2".into(),
            },
            Header {
                key: "seq".into(),
                value: sequence.to_string(),
            },
            Header {
                key: "message_id".into(),
                value: "m1".into(),
            },
        ],
        payload: Some(payload.to_vec()),
        ..Frame::default()
    }
}
#[test]
fn official_frame_schema_preserves_envelope_and_combines_out_of_order_parts() {
    let frame = fragment(1, b"world");
    assert_eq!(
        Frame::decode(frame.encode_to_vec().as_slice()).unwrap(),
        frame
    );
    let mut fragments = Fragments::default();
    assert!(fragments.combine(&frame).unwrap().is_none());
    assert!(fragments.combine(&frame).unwrap().is_none());
    assert_eq!(
        fragments.combine(&fragment(0, b"hello ")).unwrap().unwrap(),
        b"hello world"
    );
    assert!(fragments.0.is_empty());
}
#[test]
fn malformed_or_conflicting_fragments_never_panic_or_grow_without_bound() {
    let mut fragments = Fragments::default();
    assert!(fragments.combine(&fragment(2, b"bad")).is_err());
    fragments.combine(&fragment(0, b"first")).unwrap();
    assert!(fragments.combine(&fragment(0, b"changed")).is_err());
    let mut oversized = fragment(0, b"a");
    oversized.headers[0].value = "100000".into();
    assert_eq!(
        fragments.combine(&oversized),
        Err(ErrorCode::ResourceExhausted)
    );
}
