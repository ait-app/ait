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
