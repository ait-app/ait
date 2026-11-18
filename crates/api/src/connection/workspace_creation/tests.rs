use std::time::Duration;

use super::*;
use crate::test_support::{TOKEN, address};
use crate::{Api, Services};

#[tokio::test]
async fn replays_wait_for_the_in_flight_creation_to_finish() {
    let api = Api::new(
        address(),
        "server".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let creations = api.shared.metadata.creations.clone();
    let admission = creations
        .begin(Kind::Workspace, "replayed", json!({}))
        .unwrap();
    let composition = Composition {
        runtime: api.shared.runtime.clone(),
        metadata: api.shared.metadata.clone(),
        provider: api.shared.provider.clone(),
    };
    let waiter =
        tokio::spawn(async move { await_finished(&composition, "replayed".to_owned()).await });
    let ready = creations
        .advance(&admission.snapshot, Phase::WorkspaceReady, None, None)
        .unwrap();
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(!waiter.is_finished());
    creations
        .advance(&ready, Phase::Completed, None, None)
        .unwrap();
    let finished = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(finished.phase, Phase::Completed);
    api.begin_shutdown();
    api.wait_closed().await;
}
