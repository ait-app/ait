use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[derive(Debug, Default)]
struct Store {
    receipts: Mutex<BTreeMap<String, Receipt>>,
    fail: AtomicBool,
}

#[derive(Debug, Default)]
struct Publisher(Mutex<Vec<String>>);

impl crate::hub::EventPublisher for Publisher {
    fn publish(&self, method: &str, _: &Value) -> bool {
        self.0.lock().unwrap().push(method.to_owned());
        false
    }
}

#[test]
fn host_publisher_observes_only_committed_creation_receipts() {
    let store = Arc::new(Store::default());
    let creations = Creations::with_store(store.clone()).unwrap();
    let publisher = Arc::new(Publisher::default());
    creations.set_publisher(publisher.clone());
    store.fail.store(true, Ordering::Relaxed);
    assert!(creations.begin(Kind::Agent, "failed", json!({})).is_err());
    assert!(publisher.0.lock().unwrap().is_empty());
    store.fail.store(false, Ordering::Relaxed);
    creations.begin(Kind::Agent, "accepted", json!({})).unwrap();
    assert_eq!(
        publisher.0.lock().unwrap().as_slice(),
        &["agent.create.update"]
    );
}

impl ReceiptStore for Store {
    fn list(&self) -> Result<Vec<Receipt>, ErrorCode> {
        Ok(self.receipts.lock().unwrap().values().cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode> {
        Ok(self.receipts.lock().unwrap().get(id).cloned())
    }

    fn put(&self, receipt: Receipt) -> Result<(), ErrorCode> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(ErrorCode::RegistryIo);
        }
        self.receipts
            .lock()
            .unwrap()
            .insert(receipt.id.clone(), receipt);
        Ok(())
    }
}

#[test]
fn injected_persistence_failure_does_not_claim_resources_or_publish_progress() {
    let store = Arc::new(Store::default());
    let creations = Creations::with_store(store.clone()).unwrap();
    let (outbound, mut receiver) = Outbound::new();
    let observer = creations.observe(Kind::Agent, "one", outbound).unwrap();
    observer.activate().unwrap();
    store.fail.store(true, Ordering::SeqCst);
    let intent = json!({"agentId":"reserved-agent"});
    assert!(matches!(
        creations.begin(Kind::Agent, "one", intent.clone()),
        Err(ErrorCode::RegistryIo)
    ));
    assert!(receiver.try_recv().is_err());
    assert!(store.list().unwrap().is_empty());

    store.fail.store(false, Ordering::SeqCst);
    let admitted = creations.begin(Kind::Agent, "two", intent).unwrap();
    assert!(admitted.execute);
    assert_eq!(
        admitted.snapshot.agent_id.as_deref(),
        Some("reserved-agent")
    );
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn creation_receipts_and_progress_exclude_pending_permission_contents() {
    let store = Arc::new(Store::default());
    let creations = Creations::with_store(store.clone()).unwrap();
    let admission = creations.begin(Kind::Agent, "private", json!({})).unwrap();
    let (outbound, mut receiver) = Outbound::new();
    let observer = creations.observe(Kind::Agent, "private", outbound).unwrap();
    observer.activate().unwrap();
    let snapshot = creations
        .advance(
            &admission.snapshot,
            "agent_ready",
            Some(json!({"agent": {
                "id": admission.snapshot.agent_id,
                "pendingPermissions": [{
                    "id": "question",
                    "input": {"questions": [{
                        "secret": true, "initialValue": "private-prefill"
                    }]}
                }]
            }})),
            None,
        )
        .unwrap();
    assert_eq!(
        snapshot.agent.as_ref().unwrap()["pendingPermissions"],
        json!([])
    );
    let receipt = store
        .get(&identity(Kind::Agent, "private"))
        .unwrap()
        .unwrap();
    assert_eq!(receipt.snapshot.agent_id, admission.snapshot.agent_id);
    assert!(
        !serde_json::to_string(&receipt)
            .unwrap()
            .contains("private-prefill")
    );
    let queued = receiver.try_recv().unwrap();
    let crate::outbound::Frame::Text(text) = &queued.message else {
        panic!("expected creation progress");
    };
    let event: Value = serde_json::from_str(text).unwrap();
    assert_eq!(event["params"]["phase"], "agent_ready");
    assert_eq!(event["params"]["agent"]["pendingPermissions"], json!([]));
}

#[test]
fn creation_receipts_loaded_from_older_hosts_do_not_reexpose_pending_permissions() {
    let store = Arc::new(Store::default());
    let creations = Creations::with_store(store.clone()).unwrap();
    let admission = creations.begin(Kind::Agent, "older", json!({})).unwrap();
    let mut receipt = store.get(&identity(Kind::Agent, "older")).unwrap().unwrap();
    receipt.snapshot.phase = "completed".to_owned();
    receipt.snapshot.agent = Some(json!({
        "id": admission.snapshot.agent_id,
        "pendingPermissions": [{
            "id": "older-question",
            "input": {"questions": [{"secret": true, "initialValue": "old-private-prefill"}]}
        }]
    }));
    store.put(receipt).unwrap();
    let restarted = Creations::with_store(store).unwrap();
    let snapshot = restarted.snapshot(Kind::Agent, "older").unwrap().unwrap();
    assert_eq!(
        snapshot.agent.as_ref().unwrap()["pendingPermissions"],
        json!([])
    );
    let replay = restarted.begin(Kind::Agent, "older", json!({})).unwrap();
    assert!(!replay.execute);
    assert_eq!(replay.snapshot.agent_id, admission.snapshot.agent_id);
    assert_eq!(
        replay.snapshot.agent.unwrap()["pendingPermissions"],
        json!([])
    );
}
