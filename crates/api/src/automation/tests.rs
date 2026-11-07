use std::sync::Mutex;

use model::automation::Root;

use super::*;

#[derive(Debug, Default)]
struct MemoryStore {
    document: Mutex<Option<Vec<u8>>>,
}

impl OutboxStore for MemoryStore {
    fn load(&self) -> Result<Value, StoreError> {
        match &*self.document.lock().expect("document") {
            Some(bytes) => serde_json::from_slice(bytes).map_err(|_| StoreError),
            None => Ok(Value::Null),
        }
    }

    fn save(&self, document: &[u8]) -> Result<(), StoreError> {
        *self.document.lock().expect("document") = Some(document.to_vec());
        Ok(())
    }
}

fn enabled() -> Settings {
    Settings {
        enabled: true,
        roots: vec![Root {
            path: "/work".to_owned(),
            label: None,
        }],
        providers: Vec::new(),
    }
}

#[test]
fn settings_persist_and_reload() {
    let store = Arc::new(MemoryStore::default());
    let automation = Automation::default();
    automation.open(store.clone()).expect("empty store opens");
    assert_eq!(automation.get(), Settings::default());
    automation.set(enabled()).expect("saved");
    let reloaded = Automation::default();
    reloaded.open(store).expect("saved document opens");
    assert_eq!(reloaded.get(), enabled());
    assert!(
        reloaded.open(Arc::new(MemoryStore::default())).is_err(),
        "one store only"
    );
}

#[test]
fn invalid_documents_and_settings_are_refused() {
    let store = MemoryStore::default();
    store
        .save(br#"{"enabled":true,"roots":[{"path":"relative"}]}"#)
        .expect("raw save");
    assert!(Automation::default().open(Arc::new(store)).is_err());
    let automation = Automation::default();
    let mut invalid = enabled();
    invalid.roots[0].path = "relative".to_owned();
    assert_eq!(automation.set(invalid), Err(ErrorCode::InvalidMessage));
    assert_eq!(automation.get(), Settings::default());
}

#[test]
fn sessions_are_admitted_while_enabled_and_cancelled_on_disable() {
    let automation = Automation::default();
    assert_eq!(automation.admit().err(), Some(ErrorCode::Forbidden));
    automation.set(enabled()).expect("enable");
    let first = automation.admit().expect("admitted");
    automation.set(enabled()).expect("unchanged");
    assert!(
        !first.cancellation().is_cancelled(),
        "only disabling ends sessions"
    );
    automation.set(Settings::default()).expect("disable");
    assert!(first.cancellation().is_cancelled());
    automation.set(enabled()).expect("enable again");
    let second = automation.admit().expect("admitted again");
    assert!(!second.cancellation().is_cancelled());
}

#[test]
fn subscribers_see_changes() {
    let automation = Automation::default();
    let mut settings = automation.subscribe();
    automation.set(enabled()).expect("enable");
    assert!(settings.has_changed().expect("sender alive"));
    assert!(settings.borrow_and_update().enabled);
}
