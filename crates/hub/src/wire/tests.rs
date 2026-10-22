use super::*;
use serde_json::json;

fn frame(header: &serde_json::Value, payload: &[u8]) -> Bytes {
    let header = header.to_string();
    let mut bytes = u16::try_from(header.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(payload);
    bytes.into()
}

#[test]
fn binary_envelopes_preserve_bytes_and_validate_limits() {
    let id = Uuid::new_v4();
    let bytes = frame(
        &json!({"t":"msg","seq":3,"s":id,"pt":"binary"}),
        &[0, 255, 1],
    );
    let (header, body) = decode(&bytes).unwrap();
    assert!(matches!(header, Inbound::Msg { seq:3, s, .. } if s == id));
    assert_eq!(&body[..], &[0, 255, 1]);
    assert!(decode(&Bytes::from_static(&[0])).is_err());
    assert!(decode(&Bytes::from_static(&[0, 10, 1])).is_err());
    assert!(decode(&frame(&json!({"t":"ack","a":1}), &vec![0; MAX_PAYLOAD + 1])).is_err());
    assert!(decode(&frame(&json!({"t":"future"}), &[])).is_err());
    assert!(message(PayloadKind::Text, Bytes::from_static(&[255])).is_err());
    assert_eq!(
        message(PayloadKind::Binary, body).unwrap().into_data()[1],
        255
    );
}

#[test]
fn encoded_control_and_session_headers_follow_the_center_contract() {
    let bytes = encode(&Outbound::Hello { v: 1, resume: None }, &[]).unwrap();
    let n = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
    let header: serde_json::Value = serde_json::from_slice(&bytes[2..2 + n]).unwrap();
    assert_eq!(header, json!({"t":"hello","v":1,"resume":null}));
    assert!(encode(&Outbound::Ack { a: 1 }, &vec![0; MAX_PAYLOAD + 1]).is_err());
    let (header, _) = decode(&frame(&json!({"t":"error","code":"replaced"}), &[])).unwrap();
    assert!(matches!(header, Inbound::Error { code } if code == "replaced"));
}
