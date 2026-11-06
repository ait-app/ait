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
            r#"{{"t":"open","seq":2,"s":"{s}","from":"{s}","visitor":{{"user_id":"{s}","display_name":null,"group_id":"{s}","share_id":"{share}","resource_kind":"workspace","resource_key":"ws_1","permission":"operate"}}}}"#
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
        (visitor.share_id, visitor.resource_key.as_str()),
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

#[test]
fn offers_only_implemented_features_the_center_declared() {
    assert_eq!(offer(None, true), None);
    let declared = ["hosts", "download", "lease", "future"].map(str::to_owned);
    assert_eq!(
        offer(Some(&declared), true),
        Some(vec!["lease", "download"])
    );
    assert_eq!(offer(Some(&[]), true), Some(Vec::new()));
}

#[test]
fn bulk_is_offered_only_when_a_bulk_link_can_be_opened() {
    let declared = ["lease", "credit", "download", "bulk"].map(str::to_owned);
    assert_eq!(
        offer(Some(&declared), true),
        Some(vec!["lease", "credit", "download", "bulk"])
    );
    // Without bulk tickets the main link never asks for a bulk link.
    assert_eq!(
        offer(Some(&declared), false),
        Some(vec!["lease", "credit", "download"])
    );
    // A bulk link needs both `credit` and `download`.
    let partial = ["download", "bulk"].map(str::to_owned);
    assert_eq!(offer(Some(&partial), true), Some(vec!["download"]));
    assert_eq!(bulk_offer(&partial), None);
    assert_eq!(bulk_offer(&declared), Some(vec!["credit", "download"]));
}

#[test]
fn decodes_lane_hints_as_unsequenced() {
    let (hint, _) = decode(&raw(r#"{"t":"lane","lane":"bulk"}"#, b"")).expect("lane");
    assert!(matches!(&hint, Inbound::Lane { lane } if lane == "bulk"));
    assert_eq!(hint.seq(), None);
    let (other, _) = decode(&raw(r#"{"t":"lane","lane":"future"}"#, b"")).expect("unknown lane");
    assert!(matches!(other, Inbound::Lane { .. }));
}

#[test]
fn decodes_feature_fields_and_frames() {
    let s = Uuid::new_v4();
    let (open, _) = decode(&raw(
        &format!(r#"{{"t":"open","seq":1,"s":"{s}","from":"{s}","k":"download","w":65536}}"#),
        b"",
    ))
    .expect("open");
    assert!(matches!(
        open,
        Inbound::Open {
            k: SessionKind::Download,
            w: Some(65_536),
            ..
        }
    ));
    let (open, _) = decode(&raw(
        &format!(r#"{{"t":"open","seq":1,"s":"{s}","k":"tunnel"}}"#),
        b"",
    ))
    .expect("unknown kinds decode");
    assert!(matches!(
        open,
        Inbound::Open {
            k: SessionKind::Unsupported,
            w: None,
            ..
        }
    ));
    let (credit, _) =
        decode(&raw(&format!(r#"{{"t":"credit","s":"{s}","c":7}}"#), b"")).expect("credit");
    assert!(matches!(credit, Inbound::Credit { c: 7, .. }));
    assert_eq!(credit.seq(), None);
    let link = Uuid::new_v4();
    let (welcome, _) = decode(&raw(
        &format!(
            r#"{{"t":"welcome","link":"{link}","resumed":false,"recv":0,"resume_token":"t","grace_seconds":120,"f":["lease"],"win":262144}}"#
        ),
        b"",
    ))
    .expect("welcome");
    assert!(matches!(welcome, Inbound::Welcome { f, .. } if f == ["lease"]));
}

#[test]
fn encodes_feature_fields_only_when_present() {
    let s = Uuid::new_v4();
    let header = |frame: &Bytes| -> serde_json::Value {
        let length = usize::from(u16::from_be_bytes([frame[0], frame[1]]));
        serde_json::from_slice(&frame[2..2 + length]).expect("json header")
    };
    let plain = encode(&Outbound::Accept { seq: 1, s, w: None }, b"").expect("accept");
    assert_eq!(
        header(&plain),
        serde_json::json!({"t":"accept","seq":1,"s":s})
    );
    let windowed = encode(
        &Outbound::Accept {
            seq: 1,
            s,
            w: Some(65_536),
        },
        b"",
    )
    .expect("accept");
    assert_eq!(header(&windowed)["w"], 65_536);
    let credit = encode(&Outbound::Credit { s, c: 9 }, b"").expect("credit");
    assert_eq!(
        header(&credit),
        serde_json::json!({"t":"credit","s":s,"c":9})
    );
}
