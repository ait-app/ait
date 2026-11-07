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
fn every_delivery_is_granted_as_cumulative_credit() {
    let mut incoming = Incoming::default();

    assert_eq!(incoming.delivered(10), Some(10));
    assert_eq!(incoming.delivered(0), None, "nothing new to grant");
    assert_eq!(incoming.delivered(5), Some(15), "the credit is cumulative");
}

#[tokio::test]
async fn a_large_message_after_a_small_one_never_waits_for_credit_that_never_comes() {
    let (peer, credit) = watch::channel(0);
    let mut outgoing = Outgoing::new(credit, WINDOW);
    let mut incoming = Incoming::default();

    outgoing.admit(400_000).await.expect("fits the window");
    // The receiver delivers it; whatever it grants reaches the sender.
    if let Some(granted) = incoming.delivered(400_000) {
        peer.send_replace(granted);
    }
    timeout(Duration::from_secs(1), outgoing.admit(800_000))
        .await
        .expect("the second message is admitted")
        .expect("admitted");
}
