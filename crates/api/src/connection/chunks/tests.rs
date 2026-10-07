use super::*;

fn frame(id: u32, size: u32, offset: u32, data: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x30];
    frame.extend_from_slice(&id.to_be_bytes());
    frame.extend_from_slice(&size.to_be_bytes());
    frame.extend_from_slice(&offset.to_be_bytes());
    frame.extend_from_slice(data);
    frame
}

#[test]
fn assembles_only_complete_ordered_messages_and_releases_budget() {
    let mut assembly = Assembly::default();
    let (ack, complete) = assembly.push(&frame(7, 6, 0, b"abc")).unwrap();
    assert_eq!(ack["offset"], 3);
    assert!(complete.is_none());
    let (_, complete) = assembly.push(&frame(7, 6, 3, b"def")).unwrap();
    let complete = complete.unwrap();
    assert_eq!(complete.bytes, b"abcdef");
    assert!(assembly.pending.is_none());
    assert_eq!(complete.permit.num_permits(), 6);
}

#[test]
fn rejects_bad_headers_order_identity_lengths_and_expiration() {
    for invalid in [
        vec![0x30],
        frame(1, 0, 0, b"a"),
        frame(1, u32::try_from(MAX_BYTES).unwrap() + 1, 0, b"a"),
        frame(1, 3, 1, b"a"),
    ] {
        assert!(Assembly::default().push(&invalid).is_err());
    }
    for invalid in [
        frame(2, 6, 3, b"d"),
        frame(1, 7, 3, b"d"),
        frame(1, 6, 0, b"a"),
        frame(1, 6, 3, b"long"),
    ] {
        let mut assembly = Assembly::default();
        assembly.push(&frame(1, 6, 0, b"abc")).unwrap();
        assert!(assembly.push(&invalid).is_err());
    }
    let mut assembly = Assembly::default();
    assembly.push(&frame(1, 6, 0, b"abc")).unwrap();
    assembly.pending.as_mut().unwrap().deadline = Instant::now();
    assert!(assembly.push(&frame(1, 6, 3, b"def")).is_err());
}
