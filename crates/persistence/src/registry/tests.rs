use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

mod performance;

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
struct Record {
    id: String,
    value: f64,
}

fn registry(path: PathBuf) -> FileRegistry<Record> {
    FileRegistry::new(path, |record| &record.id)
}

fn record(id: &str, value: f64) -> Record {
    Record {
        id: id.to_owned(),
        value,
    }
}

fn insert(registry: &FileRegistry<Record>, record: Record) -> Result<(), Error> {
    registry.mutate(|records| {
        records.insert(record.id.clone(), record);
        Ok(((), true))
    })
}

#[test]
fn shared_views_reuse_commits_and_survive_updates_removals_and_failed_writes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    let mut store = registry(path.clone());
    insert(&store, record("first", 1.0)).unwrap();
    insert(&store, record("second", 2.0)).unwrap();
    let before = store.snapshot().unwrap();
    assert!(Arc::ptr_eq(&before, &store.snapshot().unwrap()));
    store.mutate(|_| Ok(((), false))).unwrap();
    assert!(Arc::ptr_eq(&before, &store.snapshot().unwrap()));
    insert(&store, record("first", 3.0)).unwrap();
    let updated = store.snapshot().unwrap();
    assert!(!Arc::ptr_eq(&before, &updated));
    assert_eq!(*before[0], record("first", 1.0));
    assert_eq!(*updated[0], record("first", 3.0));
    assert_eq!(*updated[1], record("second", 2.0));
    store.set_writer(Arc::new(|_, _| Err(Error::Io)));
    assert_eq!(insert(&store, record("third", 4.0)), Err(Error::Io));
    assert!(Arc::ptr_eq(&updated, &store.snapshot().unwrap()));
    let reopened = registry(path);
    assert_eq!(reopened.list().unwrap(), store.list().unwrap());
    reopened
        .mutate(|records| {
            records.shift_remove("first");
            Ok(((), true))
        })
        .unwrap();
    assert_eq!(*reopened.snapshot().unwrap()[0], record("second", 2.0));
    assert_eq!(updated.len(), 2);
}

#[test]
fn lazy_load_keeps_order_duplicate_replacement_and_missing_file_semantics() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    let store = registry(path.clone());
    store.initialize().unwrap();
    assert!(!store.exists());
    assert!(store.list().unwrap().is_empty());
    assert_eq!(store.get("absent").unwrap(), None);
    assert!(format!("{store:?}").contains("records.json"));
    insert(&store, record("first", 1.0)).unwrap();
    insert(&store, record("second", 2.0)).unwrap();
    assert!(store.exists());
    assert_eq!(
        registry(path.clone()).list().unwrap(),
        store.list().unwrap()
    );
    crate::File::new(&path)
        .write(
            &serde_json::to_vec(&[
                record("first", 1.0),
                record("second", 2.0),
                record("first", 3.0),
            ])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        registry(path).list().unwrap(),
        [record("first", 3.0), record("second", 2.0)]
    );
}

#[test]
fn invalid_files_records_and_failed_writers_preserve_committed_state() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    let file = crate::File::new(&path);
    file.write(b"broken").unwrap();
    let mut store = registry(path.clone());
    assert_eq!(store.initialize(), Err(Error::InvalidFile));
    assert_eq!(insert(&store, record("one", 1.0)), Err(Error::InvalidFile));
    assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    file.write(b"[]").unwrap();
    store.initialize().unwrap();
    insert(&store, record("one", 1.0)).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        insert(&store, record("bad", f64::NAN)),
        Err(Error::InvalidRecord)
    );
    let original = store.writer();
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    store.set_writer(Arc::new(move |path, bytes| {
        if flag.load(Ordering::SeqCst) {
            Err(Error::Io)
        } else {
            original(path, bytes)
        }
    }));
    assert_eq!(insert(&store, record("two", 2.0)), Err(Error::Io));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(store.list().unwrap(), [record("one", 1.0)]);
    fail.store(false, Ordering::SeqCst);
    insert(&store, record("two", 2.0)).unwrap();
    store.freeze().unwrap();
    assert_eq!(insert(&store, record("three", 3.0)), Err(Error::Frozen));
    assert_eq!(store.list().unwrap().len(), 2);
}

#[test]
fn oversized_mutations_leave_committed_state_and_hooks_untouched() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    let store = registry(path.clone());
    insert(&store, record("one", 1.0)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let snapshot = store.snapshot().unwrap();
    let oversized = record(
        &"x".repeat(usize::try_from(MAX_REGISTRY_BYTES).unwrap()),
        2.0,
    );
    let result = store.mutate_with(
        |records| {
            records.insert(oversized.id.clone(), oversized);
            Ok(((), true))
        },
        |_| panic!("oversized records must not create a journal"),
        || panic!("oversized records must not be committed"),
    );
    assert_eq!(result, Err(Error::InvalidRecord));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(Arc::ptr_eq(&snapshot, &store.snapshot().unwrap()));
    assert_eq!(registry(path).list().unwrap(), [record("one", 1.0)]);
}

#[test]
fn oversized_existing_registries_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_REGISTRY_BYTES + 1)
        .unwrap();
    assert_eq!(registry(path).initialize(), Err(Error::InvalidFile));
}

#[test]
fn hooks_capture_locked_before_image_and_failed_post_commit_requires_owner_recovery() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("records.json");
    let store = registry(path.clone());
    insert(&store, record("one", 1.0)).unwrap();
    let update = |records: &mut IndexMap<String, Record>| {
        records.insert("two".to_owned(), record("two", 2.0));
        Ok(((), true))
    };
    assert_eq!(
        store.mutate_with(update, |_| Err(Error::Io), || Ok(())),
        Err(Error::Io)
    );
    assert_eq!(registry(path.clone()).list().unwrap(), [record("one", 1.0)]);
    assert_eq!(
        store.mutate_with(
            update,
            |before| {
                assert_eq!(
                    before.values().map(AsRef::as_ref).collect::<Vec<_>>(),
                    [&record("one", 1.0)]
                );
                Ok(())
            },
            || Err(Error::Io)
        ),
        Err(Error::Io)
    );
    assert_eq!(store.list().unwrap(), [record("one", 1.0)]);
    assert_eq!(registry(path).list().unwrap().len(), 2);
}

#[test]
fn serialized_mutations_keep_concurrent_updates_and_noops_skip_writes() {
    let root = tempfile::tempdir().unwrap();
    let store = registry(root.path().join("records.json"));
    insert(&store, record("counter", 0.0)).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let store = &store;
            scope.spawn(move || {
                for _ in 0..10 {
                    store
                        .mutate(|records| {
                            records.get_mut("counter").unwrap().value += 1.0;
                            Ok(((), true))
                        })
                        .unwrap();
                }
            });
        }
    });
    assert_eq!(
        store.get("counter").unwrap().unwrap().value.to_bits(),
        40.0_f64.to_bits()
    );
    store
        .mutate_with(
            |_| Ok(((), false)),
            |_| panic!("no-op hook"),
            || panic!("no-op hook"),
        )
        .unwrap();
}

#[test]
fn custom_errors_and_poisoned_locks_preserve_storage_error_categories() {
    #[derive(Debug, PartialEq)]
    struct CustomError(Error);
    impl From<Error> for CustomError {
        fn from(error: Error) -> Self {
            Self(error)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let store: FileRegistry<Record, CustomError> =
        FileRegistry::new(root.path().join("custom.json"), |record| &record.id);
    store.freeze().unwrap();
    assert_eq!(
        store.mutate(|_| Ok(((), true))),
        Err(CustomError(Error::Frozen))
    );
    let store = registry(root.path().join("poisoned.json"));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), Error> = store.mutate(|_| panic!("poison the commit lock"));
    }));
    assert_eq!(store.list(), Err(Error::Frozen));
    assert_eq!(
        registry(root.path().to_owned()).initialize(),
        Err(Error::Io)
    );
}
