use std::hint::black_box;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use super::*;

#[derive(Debug, Serialize, serde::Deserialize)]
struct TrackedRecord {
    id: String,
    payload: String,
    #[serde(skip)]
    clones: Arc<AtomicUsize>,
}

impl Clone for TrackedRecord {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::Relaxed);
        Self {
            id: self.id.clone(),
            payload: self.payload.clone(),
            clones: self.clones.clone(),
        }
    }
}

#[test]
fn repeated_committed_snapshot_reads_avoid_all_payload_clones() {
    const RECORDS: usize = 257;
    const READS: usize = 200;
    let root = tempfile::tempdir().unwrap();
    let store: FileRegistry<TrackedRecord> =
        FileRegistry::new(root.path().join("records.json"), |record| &record.id);
    let clones = Arc::new(AtomicUsize::new(0));
    store
        .mutate(|records| {
            for index in 0..RECORDS {
                let id = index.to_string();
                records.insert(
                    id.clone(),
                    TrackedRecord {
                        id,
                        payload: "x".repeat(3 * 1024),
                        clones: clones.clone(),
                    },
                );
            }
            Ok(((), true))
        })
        .unwrap();
    clones.store(0, Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..READS {
        let records = store.list().unwrap();
        black_box(&records);
    }
    let owned_elapsed = start.elapsed();
    let owned_clones = clones.swap(0, Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..READS {
        let records = store.snapshot().unwrap();
        assert_eq!(records.len(), RECORDS);
        black_box(&records);
    }
    let shared_elapsed = start.elapsed();
    let shared_clones = clones.load(Ordering::Relaxed);
    assert_eq!(owned_clones, RECORDS * READS);
    assert_eq!(shared_clones, 0);
    for _ in 0..READS {
        let record = store.get_shared("0").unwrap().unwrap();
        black_box(&record);
    }
    assert_eq!(clones.load(Ordering::Relaxed), 0);
    assert!(store.get_shared("absent").unwrap().is_none());
    eprintln!(
        "registry reads={READS} records={RECORDS} payload_bytes=3072 \
         owned_clones={owned_clones} shared_clones={shared_clones} owned_us={} shared_us={}",
        owned_elapsed.as_micros(),
        shared_elapsed.as_micros()
    );
}
