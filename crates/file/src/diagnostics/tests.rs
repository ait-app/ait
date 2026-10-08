use super::*;

#[test]
fn retention_enforces_count_bytes_age_and_preserves_unowned_files() {
    let directory = tempfile::tempdir().unwrap();
    let now = Utc::now();
    for index in 0..25 {
        save(directory.path(), &format!("fixture {index}"), now).unwrap();
    }
    std::fs::write(directory.path().join("unowned.txt"), "keep").unwrap();
    assert_eq!(files(directory.path()).unwrap().len(), COUNT_LIMIT);
    let report = recent(directory.path());
    assert_eq!(report.matches("--- Saved incident ---").count(), 3);
    prune(directory.path(), now + chrono::Duration::days(8)).unwrap();
    assert!(files(directory.path()).unwrap().is_empty());
    assert!(directory.path().join("unowned.txt").exists());
    let path = directory
        .path()
        .join(format!("incident-{}.txt", uuid::Uuid::new_v4()));
    std::fs::File::create(&path)
        .unwrap()
        .set_len(BYTE_LIMIT + 1)
        .unwrap();
    let retained = directory
        .path()
        .join(format!("incident-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&retained, "small useful incident").unwrap();
    prune(directory.path(), now).unwrap();
    assert!(!path.exists());
    assert!(retained.exists());
}

#[test]
fn unreadable_storage_is_reported_and_missing_storage_is_empty() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    assert!(recent(&missing).contains("Saved incidents: 0"));
    std::fs::write(&missing, "file").unwrap();
    assert!(recent(&missing).contains("unavailable"));
    assert!(save(&missing, "report", Utc::now()).is_err());
}

#[test]
fn oversized_incidents_are_rejected_without_creating_storage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("evidence");
    assert!(matches!(
        save(&path, &"x".repeat(REPORT_LIMIT + 1), Utc::now()),
        Err(crate::Error::TooLarge)
    ));
    assert!(!path.exists());
}
