use super::*;

#[test]
fn only_confirmed_outcomes_are_terminal() {
    for status in [ObservedStatus::Running, ObservedStatus::Unknown] {
        assert!(!status.is_terminal());
    }
    for status in [
        ObservedStatus::Completed,
        ObservedStatus::Failed,
        ObservedStatus::Canceled,
    ] {
        assert!(status.is_terminal());
        let encoded = serde_json::to_value(status).expect("status serializes");
        assert_eq!(
            serde_json::from_value::<ObservedStatus>(encoded).expect("status round trip"),
            status
        );
    }
}
