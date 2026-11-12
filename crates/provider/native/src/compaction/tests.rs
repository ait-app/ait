use super::*;

#[test]
fn compaction_interruption_fails_only_open_operations_once() {
    let mut compactions = Compactions::default();
    compactions.marker("open", "loading", None).unwrap();
    compactions.marker("closed", "completed", None).unwrap();
    assert_eq!(
        compactions.fail_pending(),
        [json!({"type":"compaction","compactionId":"open","status":"failed"})]
    );
    assert_eq!(compactions.fail_pending(), [] as [Value; 0]);
}

#[test]
fn compaction_lifecycle_deduplicates_starts_and_terminal_replays() {
    for terminal in ["completed", "failed"] {
        let mut compactions = Compactions::default();
        let started = compactions
            .marker("native", "loading", Some("manual"))
            .unwrap()
            .unwrap();
        assert_eq!(started["compactionId"], "native");
        assert_eq!(started["trigger"], "manual");
        assert!(
            compactions
                .marker("native", "loading", None)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            compactions
                .marker("native", terminal, None)
                .unwrap()
                .unwrap()["status"],
            terminal
        );
        assert!(
            compactions
                .marker("native", terminal, None)
                .unwrap()
                .is_none()
        );
        assert!(
            compactions
                .marker("native", "loading", None)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn compaction_terminal_only_history_is_valid_and_state_is_bounded() {
    let mut compactions = Compactions::default();
    assert!(
        compactions
            .marker("history", "completed", None)
            .unwrap()
            .is_some()
    );
    for (id, status, trigger) in [
        ("", "loading", None),
        ("id", "unknown", None),
        ("id", "loading", Some("unknown")),
    ] {
        assert!(compactions.marker(id, status, trigger).is_err());
    }
    assert!(
        compactions
            .marker(&"x".repeat(513), "loading", None)
            .is_err()
    );
    compactions.states = (0..MAX_ITEMS)
        .map(|id| (id.to_string(), "loading"))
        .collect();
    assert!(compactions.marker("extra", "loading", None).is_err());
    assert!(
        compactions
            .marker("0", "completed", None)
            .unwrap()
            .is_some()
    );
}
