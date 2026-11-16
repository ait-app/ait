use super::*;
use model::ErrorCode;

#[test]
fn title_backfill_preserves_explicit_and_internal_titles_and_propagates_storage_errors() {
    let (manager, registry, _) = make_manager();
    manager.recover_titles().unwrap();
    manager.fill_missing_title("absent", None).unwrap();
    assert_eq!(
        manager.fill_missing_title("absent", Some("Title".to_owned())),
        Err(ErrorCode::AgentNotFound)
    );

    let mut record = stored_record();
    for (title, internal, expected) in [
        (Some("Existing"), false, Some("Existing")),
        (None, true, None),
        (Some(" \n "), false, Some("Recovered")),
    ] {
        record.title = title.map(str::to_owned);
        record.internal = internal;
        registry.upsert(&record).unwrap();
        manager
            .fill_missing_title(&record.id, Some("Recovered".to_owned()))
            .unwrap();
        assert_eq!(
            registry.get(&record.id).unwrap().unwrap().title.as_deref(),
            expected
        );
    }
    registry.0.lock().unwrap().fail_update = true;
    assert_eq!(
        manager.fill_missing_title(&record.id, Some("Title".to_owned())),
        Err(ErrorCode::AgentIo)
    );
}
