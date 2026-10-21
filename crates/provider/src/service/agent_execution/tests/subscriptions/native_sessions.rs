//! Native enumeration uses bounded response admission without holding a connection's dispatcher.

use super::*;

struct ListingGate(std::path::PathBuf);

impl ListingGate {
    fn new(fixture: &Fixture) -> Self {
        fixture.mode("delayed-listing");
        Self(fixture.cwd.clone())
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("listing-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("native listing reached its gate");
    }

    fn release(&self) {
        std::fs::write(self.0.join("release-listing"), "").unwrap();
    }
}

impl Drop for ListingGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-listing"), "");
    }
}

async fn dispatch_listing(peer: &mut Peer, id: &str, params: Value) {
    let mut context = Some(Context {
        request: Request {
            id: id.to_owned(),
            method: "provider.sessions.recent.list.request".to_owned(),
            params,
        },
        runtime: &peer.state.runtime,
        outbound: &peer.outbound,
        available_subscriptions: 8,
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        crate::dispatch::dispatch(&mut context, &peer.state, &mut peer.connection),
    )
    .await
    .expect("listing must return to the connection dispatcher before native I/O completes")
    .unwrap();
    assert!(context.is_none());
}

async fn response(peer: &mut Peer) -> Value {
    let queued = tokio::time::timeout(Duration::from_secs(3), peer.receiver.recv())
        .await
        .expect("independent listing response arrived")
        .unwrap();
    let Frame::Text(text) = &queued.message else {
        panic!("expected a JSON listing response");
    };
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn native_session_listing_dispatch_keeps_other_listings_and_errors_responsive() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = ListingGate::new(&fixture);
    let mut peer = Peer::new(&execution);

    dispatch_listing(
        &mut peer,
        "slow",
        json!({"providers":["codex"],"cwd":fixture.cwd,"limit":15}),
    )
    .await;
    gate.entered().await;
    dispatch_listing(&mut peer, "fast", json!({"providers":[],"limit":15})).await;
    let fast = response(&mut peer).await;
    assert_eq!(fast["request_id"], "fast");
    assert_eq!(fast["result"]["entries"], json!([]));
    dispatch_listing(&mut peer, "invalid", json!({"limit":0})).await;
    let invalid = response(&mut peer).await;
    assert_eq!(invalid["request_id"], "invalid");
    assert_eq!(invalid["code"], "invalid_message");
    assert!(peer.receiver.try_recv().is_err());

    gate.release();
    let slow = response(&mut peer).await;
    assert_eq!(slow["request_id"], "slow");
    assert_eq!(slow["result"]["entries"], json!([]));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_session_listing_response_slots_remain_bounded_through_delivery() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let mut responses = Vec::new();
    for _ in 0..64 {
        responses.push(
            execution
                .admit_catalog(
                    "provider.sessions.recent.list.request",
                    json!({"providers":[]}),
                )
                .unwrap(),
        );
    }
    for response in &mut responses {
        assert_eq!(response.receive().await.unwrap()["entries"], json!([]));
    }
    assert!(matches!(
        execution.admit_catalog(
            "provider.sessions.recent.list.request",
            json!({"providers":[]})
        ),
        Err(ErrorCode::CatalogBusy)
    ));
    drop(responses);
    assert_eq!(
        execution
            .execute(
                "provider.sessions.recent.list.request",
                json!({"providers":[]})
            )
            .await
            .unwrap()["entries"],
        json!([])
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn disconnected_listing_stays_tracked_until_shutdown_finishes_native_work() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = ListingGate::new(&fixture);
    let mut peer = Peer::new(&execution);
    dispatch_listing(
        &mut peer,
        "detached",
        json!({"providers":["codex"],"cwd":fixture.cwd}),
    )
    .await;
    gate.entered().await;
    drop(peer);
    let stopping = execution.clone();
    let shutdown = tokio::spawn(async move { stopping.shutdown().await });
    execution.0.cancellation.cancelled().await;
    assert!(!shutdown.is_finished());
    gate.release();
    tokio::time::timeout(Duration::from_secs(3), shutdown)
        .await
        .expect("shutdown drains accepted native listing after disconnect")
        .unwrap()
        .unwrap();
    assert!(execution.0.thread.lock().unwrap().is_none());
}
