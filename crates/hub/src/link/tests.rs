use super::*;
use crate::wire::PayloadKind;

fn welcome(link: Uuid, node: Uuid, resumed: bool, recv: u64) -> Welcome {
    Welcome {
        v: 1,
        link,
        node_id: node,
        resumed,
        recv,
        resume_token: (!resumed).then(|| "a".repeat(64).into()),
        grace_seconds: 120,
    }
}

#[test]
fn reconnect_replays_unacknowledged_frames_without_changing_sequences() {
    let mut link = Link::default();
    let id = Uuid::new_v4();
    let node = Uuid::new_v4();
    link.welcome(welcome(id, node, false, 0)).unwrap();
    let s = Uuid::new_v4();
    for n in 0..3 {
        link.push(s, &[n], |seq| Outbound::Msg {
            seq,
            s,
            pt: PayloadKind::Binary,
        })
        .unwrap();
        let (seq, _) = link.next().unwrap();
        link.sent(seq);
    }
    link.ack(1).unwrap();
    assert_eq!(link.queue.len(), 2);
    assert!(link.receive(2).unwrap());
    assert!(!link.receive(2).unwrap());
    assert!(
        link.receive(8).unwrap(),
        "center-side coalescing can leave gaps"
    );
    link.welcome(welcome(id, node, true, 1)).unwrap();
    assert_eq!(link.next().unwrap().0, 2);
    assert_eq!(link.resume.unwrap().recv, 8);
    assert!(
        link.welcome(welcome(Uuid::new_v4(), node, true, 1))
            .is_err()
    );
    assert!(link.ack(10).is_err());
    assert!(link.receive(0).is_err());
}

#[test]
fn overflow_close_discards_only_the_failed_session_and_keeps_other_replay() {
    let mut link = Link::default();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    link.push(b, b"keep", |seq| Outbound::Accept { seq, s: b })
        .unwrap();
    let body = vec![0; wire::MAX_PAYLOAD];
    while link
        .push(a, &body, |seq| Outbound::Msg {
            seq,
            s: a,
            pt: PayloadKind::Binary,
        })
        .is_ok()
    {}
    link.close(a, "overflow").unwrap();
    assert_eq!(link.queue.len(), 2);
    assert_eq!(link.queue.front().unwrap().session, b);
    assert!(link.bytes < 1000);
}

#[test]
fn rejects_missing_credentials_wrong_versions_and_invalid_resume_floors() {
    let mut link = Link::default();
    let id = Uuid::new_v4();
    let node = Uuid::new_v4();
    let mut invalid = welcome(id, node, false, 0);
    invalid.resume_token = None;
    assert!(link.welcome(invalid).is_err());
    let mut invalid = welcome(id, node, false, 0);
    invalid.v = 2;
    assert!(link.welcome(invalid).is_err());
    link.welcome(welcome(id, node, false, 0)).unwrap();
    link.push(id, &[], |seq| Outbound::Accept { seq, s: id })
        .unwrap();
    link.sent(1);
    link.ack(1).unwrap();
    assert!(link.welcome(welcome(id, node, true, 0)).is_err());
}
