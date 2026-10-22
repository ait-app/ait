use super::*;
use serde_json::json;

#[test]
fn missing_atomic_round_trip_and_malformed_journal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hub/outbox.json");
    let store = FileOutbox::new(&path);
    assert_eq!(store.load().unwrap(), Value::Null);
    let document = json!({"version":1,"events":[{"eventId":"stable"}]});
    store.save(&document).unwrap();
    assert_eq!(FileOutbox::new(&path).load().unwrap(), document);
    std::fs::write(&path, b"broken").unwrap();
    assert_eq!(store.load(), Err(StoreError));
    assert_eq!(std::fs::read(&path).unwrap(), b"broken");
}

#[test]
fn oversized_and_unwritable_storage_preserve_the_previous_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.json");
    let store = FileOutbox::new(&path);
    store.save(&json!({"good":true})).unwrap();
    assert_eq!(
        store.save(&json!("x".repeat(usize::try_from(MAX_BYTES).unwrap()))),
        Err(StoreError)
    );
    assert_eq!(store.load().unwrap(), json!({"good":true}));
    assert_eq!(FileOutbox::new(directory.path()).load(), Err(StoreError));
    assert_eq!(
        FileOutbox::new(directory.path()).save(&Value::Null),
        Err(StoreError)
    );
}
