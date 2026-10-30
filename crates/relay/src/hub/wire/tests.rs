use super::*;

fn raw(header: &str, payload: &[u8]) -> Bytes {
    let mut frame = u16::try_from(header.len())
        .expect("short header")
        .to_be_bytes()
        .to_vec();
    frame.extend_from_slice(header.as_bytes());
    frame.extend_from_slice(payload);
    frame.into()
}

#[test]
fn decodes_center_frames_and_tolerates_new_header_fields() {
    let s = Uuid::new_v4();
    let frame = raw(
        &format!(r#"{{"t":"msg","seq":4,"s":"{s}","pt":"binary","future":1}}"#),
        &[1, 2],
    );

    let (header, payload) = decode(&frame).expect("valid frame");

    assert!(matches!(
        header,
        Inbound::Msg { seq: 4, s: id, pt: PayloadKind::Binary } if id == s
    ));
    assert_eq!(header.seq(), Some(4));
    assert_eq!(payload.as_ref(), &[1, 2]);
}

#[test]
fn rejects_truncated_and_unknown_frames() {
    assert!(decode(&Bytes::from_static(&[0])).is_err());
    assert!(decode(&raw(r#"{"t":"presence","seq":1}"#, b"")).is_err());
    let mut short = 10_u16.to_be_bytes().to_vec();
    short.extend_from_slice(b"{}");
    assert!(decode(&short.into()).is_err());
}

#[test]
fn encodes_length_prefixed_header_and_payload() {
    let s = Uuid::new_v4();

    let frame = encode(
        &Outbound::Msg {
            seq: 9,
            s,
            pt: PayloadKind::Text,
        },
        b"hi",
    )
    .expect("encodable");

    let length = usize::from(u16::from_be_bytes([frame[0], frame[1]]));
    let header: serde_json::Value =
        serde_json::from_slice(&frame[2..2 + length]).expect("json header");
    assert_eq!(
        header,
        serde_json::json!({"t":"msg","seq":9,"s":s,"pt":"text"})
    );
    assert_eq!(&frame[2 + length..], b"hi");
    assert!(encode(&Outbound::Ack { a: 1 }, &vec![0; MAX_PAYLOAD + 1]).is_err());
}

#[test]
fn decodes_guest_opens_and_share_status() {
    let s = Uuid::new_v4();
    let share = Uuid::new_v4();
    let frame = raw(
        &format!(
            r#"{{"t":"open","seq":2,"s":"{s}","from":"{s}","visitor":{{"user_id":"{s}","display_name":null,"organization_id":"{s}","share_id":"{share}","workspace_key":"ws_1","permission":"operate"}}}}"#
        ),
        b"hello",
    );
    let (header, _) = decode(&frame).expect("guest open");
    let Inbound::Open {
        seq: 2,
        visitor: Some(visitor),
        ..
    } = header
    else {
        std::panic!("expected a guest open, got {header:?}");
    };
    assert_eq!(
        (visitor.share_id, visitor.workspace_key.as_str()),
        (share, "ws_1")
    );

    let (owner, _) = decode(&raw(
        &format!(r#"{{"t":"open","seq":3,"s":"{s}","from":"{s}"}}"#),
        b"",
    ))
    .expect("owner open");
    assert!(matches!(owner, Inbound::Open { visitor: None, .. }));

    let (status, payload) = decode(&raw(r#"{"t":"share_status","seq":5}"#, b"{}")).expect("status");
    assert!(matches!(status, Inbound::ShareStatus { seq: 5 }));
    assert_eq!(status.seq(), Some(5));
    assert_eq!(payload.as_ref(), b"{}");
}
