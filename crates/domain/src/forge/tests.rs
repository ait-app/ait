use super::{CheckStatus, PullState};

#[test]
fn forge_check_statuses_normalize_to_four_outcomes() {
    use crate::workspace::protocol::workspace::CheckStatus as Forge;
    assert_eq!(
        CheckStatus::from_forge(Forge::Success),
        CheckStatus::Success
    );
    assert_eq!(
        CheckStatus::from_forge(Forge::Failure),
        CheckStatus::Failure
    );
    assert_eq!(
        CheckStatus::from_forge(Forge::Cancelled),
        CheckStatus::Skipped
    );
    assert_eq!(
        CheckStatus::from_forge(Forge::Skipped),
        CheckStatus::Skipped
    );
    assert_eq!(
        CheckStatus::from_forge(Forge::Pending),
        CheckStatus::Pending
    );
}

#[test]
fn merged_and_closed_pull_requests_are_ended() {
    assert!(!PullState::Open.is_ended());
    assert!(PullState::Merged.is_ended());
    assert!(PullState::Closed.is_ended());
    assert_eq!(serde_json::to_value(PullState::Merged).unwrap(), "merged");
}
