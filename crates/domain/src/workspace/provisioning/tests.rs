use super::{GithubClonePhase, GithubCloneProgress};

#[test]
fn clone_progress_maps_each_phase_onto_the_overall_bar() {
    let cases = [
        (GithubClonePhase::Starting, None, 0),
        (GithubClonePhase::Counting, Some(0), 1),
        (GithubClonePhase::Counting, Some(100), 4),
        (GithubClonePhase::Compressing, Some(100), 8),
        (GithubClonePhase::Receiving, Some(0), 8),
        (GithubClonePhase::Receiving, Some(50), 44),
        (GithubClonePhase::Receiving, Some(100), 80),
        (GithubClonePhase::Resolving, Some(0), 80),
        (GithubClonePhase::Resolving, Some(100), 92),
        (GithubClonePhase::Checkout, Some(100), 98),
        (GithubClonePhase::Registering, None, 99),
    ];
    for (phase, percent, expected) in cases {
        assert_eq!(
            GithubCloneProgress::new(phase, percent).overall_percent(),
            expected,
            "{phase:?} at {percent:?}"
        );
    }
}

#[test]
fn clone_progress_without_a_source_percent_stays_at_the_phase_start() {
    assert_eq!(
        GithubCloneProgress::new(GithubClonePhase::Receiving, None).overall_percent(),
        8
    );
    assert_eq!(
        GithubCloneProgress::new(GithubClonePhase::Checkout, None).overall_percent(),
        92
    );
}

#[test]
fn clone_progress_clamps_out_of_range_percentages() {
    assert_eq!(
        GithubCloneProgress::new(GithubClonePhase::Receiving, Some(200)),
        GithubCloneProgress {
            phase: GithubClonePhase::Receiving,
            phase_percent: Some(100),
        }
    );
    assert_eq!(
        GithubCloneProgress::new(GithubClonePhase::Receiving, Some(200)).overall_percent(),
        80
    );
}
