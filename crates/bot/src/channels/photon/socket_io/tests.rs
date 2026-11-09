use serde_json::json;

use super::*;

#[test]
fn the_open_packet_yields_the_silence_limit() {
    let packet = decode(r#"0{"sid":"a","pingInterval":25000,"pingTimeout":20000}"#).unwrap();
    assert_eq!(packet, Packet::Open(Duration::from_secs(45)));
}

#[test]
fn control_packets_decode() {
    assert_eq!(decode("2").unwrap(), Packet::Ping);
    assert_eq!(decode("3").unwrap(), Packet::Ignored);
    assert_eq!(decode("1").unwrap(), Packet::Closed);
    assert_eq!(decode(r#"40{"sid":"b"}"#).unwrap(), Packet::Connected);
    assert_eq!(decode("41").unwrap(), Packet::Closed);
    assert_eq!(decode(r#"44{"message":"no"}"#).unwrap(), Packet::Closed);
}

#[test]
fn events_decode_with_or_without_an_ack_number() {
    let expected = Packet::Event(vec![json!("delivery"), json!({"id":"e"}), json!({"seq":7})]);
    assert_eq!(
        decode(r#"42["delivery",{"id":"e"},{"seq":7}]"#).unwrap(),
        expected
    );
    assert_eq!(
        decode(r#"4212["delivery",{"id":"e"},{"seq":7}]"#).unwrap(),
        expected
    );
}

#[test]
fn acks_carry_their_number_and_first_argument() {
    assert_eq!(
        decode(r#"430[{"ok":true}]"#).unwrap(),
        Packet::Ack(0, json!({"ok":true}))
    );
}

#[test]
fn unsupported_or_malformed_packets_are_protocol_failures() {
    for frame in ["", "x", "4", "45", "42not-json", "43[{}]", "430[]", "0{}"] {
        assert_eq!(decode(frame), Err(ErrorCode::BotIo), "{frame}");
    }
}

#[test]
fn outgoing_events_carry_their_ack_number() {
    assert_eq!(
        event(0, "subscribe", &json!({"startSeq":4})),
        r#"420["subscribe",{"startSeq":4}]"#
    );
}
