use std::time::Duration;

use tokio::time::timeout;

use super::*;

#[tokio::test]
async fn sending_stays_within_the_latest_peer_credit_plus_its_window() {
    let (peer, credit) = watch::channel(0);
    let mut outgoing = Outgoing::new(credit, 65_536);

    outgoing.admit(65_536).await.expect("a full window fits");
    assert!(
        timeout(Duration::from_millis(50), outgoing.admit(1))
            .await
            .is_err(),
        "one more byte waits for credit"
    );
    peer.send_replace(1);
    timeout(Duration::from_secs(1), outgoing.admit(1))
        .await
        .expect("credit admits it")
        .expect("admitted");

    assert_eq!(outgoing.admit(65_537).await, Err("overflow"));
    drop(peer);
    assert_eq!(outgoing.admit(1).await, Err("closed"));
}

#[test]
fn credit_is_granted_after_half_a_window_was_delivered() {
    let half = usize::try_from(WINDOW / 2).expect("window fits usize");
    let mut incoming = Incoming::default();

    assert_eq!(incoming.delivered(half - 1), None);
    assert_eq!(incoming.delivered(1), Some(u64::from(WINDOW / 2)));
    assert_eq!(incoming.delivered(half - 1), None);
    assert_eq!(
        incoming.delivered(half),
        Some(3 * u64::from(WINDOW / 2) - 1),
        "the announced credit is cumulative"
    );
}
