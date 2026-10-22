use secrecy::ExposeSecret;

use super::*;
use crate::hub::wire::PayloadKind;

fn header(frame: &Bytes) -> serde_json::Value {
    let length = usize::from(u16::from_be_bytes([frame[0], frame[1]]));
    serde_json::from_slice(&frame[2..2 + length]).expect("json header")
}

fn msg(s: Uuid) -> impl FnOnce(u64) -> Outbound {
    move |seq| Outbound::Msg {
        seq,
        s,
        pt: PayloadKind::Text,
    }
}

#[test]
fn fresh_link_sends_plain_hello_then_resumes_with_received_sequence() {
    let mut link = Link::default();
    assert_eq!(
        header(&link.hello().expect("hello")),
        serde_json::json!({"t":"hello","v":1,"resume":null})
    );
    let id = Uuid::new_v4();

    let joined = link.join(id, false, 0, Some("token".into()));

    assert_eq!(joined.expect("fresh"), Joined::Fresh);
    assert_eq!(link.token().expect("token").expose_secret(), "token");
    assert!(link.received(1));
    assert!(link.received(2));
    assert!(!link.received(2), "replayed frames are not delivered twice");
    assert_eq!(
        header(&link.hello().expect("hello"))["resume"],
        serde_json::json!({"link":id,"recv":2})
    );
}

#[test]
fn resume_replays_only_frames_the_center_lacks() {
    let mut link = Link::default();
    let id = Uuid::new_v4();
    link.join(id, false, 0, Some("token".into()))
        .expect("fresh");
    let s = Uuid::new_v4();
    let frames: Vec<Bytes> = (0..3)
        .map(|_| link.push(msg(s), b"x").expect("push"))
        .collect();
    link.acked(1).expect("ack");

    let joined = link.join(id, true, 2, None).expect("resumed");

    assert_eq!(joined, Joined::Resumed(vec![frames[2].clone()]));
    assert_eq!(header(&frames[2])["seq"], 3);
    assert!(link.acked(4).is_err(), "cannot acknowledge unsent frames");
}

#[test]
fn unknown_link_restarts_sequences() {
    let mut link = Link::default();
    link.join(Uuid::new_v4(), false, 0, Some("old".into()))
        .expect("fresh");
    link.push(msg(Uuid::new_v4()), b"x").expect("push");
    link.received(5);

    let joined = link.join(Uuid::new_v4(), false, 0, Some("new".into()));

    assert_eq!(joined.expect("fresh"), Joined::Fresh);
    assert_eq!(
        header(&link.push(msg(Uuid::new_v4()), b"").expect("push"))["seq"],
        1
    );
    assert!(link.received(1));
    assert!(
        link.join(Uuid::new_v4(), false, 0, None).is_err(),
        "a fresh link always carries a resume token"
    );
}

#[test]
fn acknowledgements_are_coalesced_and_bound_replay_memory() {
    let mut link = Link::default();
    link.join(Uuid::new_v4(), false, 0, Some("token".into()))
        .expect("fresh");
    assert_eq!(link.take_ack().expect("ack"), None);
    link.received(1);
    link.received(2);

    let ack = link.take_ack().expect("ack").expect("due");

    assert_eq!(header(&ack), serde_json::json!({"t":"ack","a":2}));
    assert_eq!(link.take_ack().expect("ack"), None);
    let s = Uuid::new_v4();
    let chunk = vec![0; wire::MAX_PAYLOAD];
    while link.has_room() {
        link.push(msg(s), &chunk).expect("push");
    }
    link.acked(link.next_seq).expect("ack all");
    assert!(link.has_room());
}
