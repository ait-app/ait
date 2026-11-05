use domain::workspace::provisioning::{GithubClonePhase, GithubCloneProgress};

use super::GitCloneProgressParser;

fn collect(chunks: &[&[u8]]) -> Vec<GithubCloneProgress> {
    let mut parser = GitCloneProgressParser::default();
    let mut updates = Vec::new();
    for chunk in chunks {
        parser.push(chunk, &mut |update| updates.push(update));
    }
    updates
}

#[test]
fn reports_and_deduplicates_each_git_phase_as_it_appears() {
    let updates = collect(&[
        b"Cloning into '.ait-clone-test'...\n",
        b"remote: Enumerating objects: 12, done.\r",
        b"remote: Counting objects: 100% (12/12), done.\r",
        b"remote: Compressing objects: 100% (12/12), done.\r",
        b"Receiving objects:  50% (6/12), 1.00 MiB | 2.00 MiB/s\r",
        b"Receiving objects:  50% (6/12), 1.00 MiB | 2.00 MiB/s\r",
        b"Receiving objects: 100% (12/12), 2.00 MiB | 2.00 MiB/s, done.\r",
        b"Resolving deltas: 100% (3/3), done.\r",
        b"Updating files: 100% (5/5), done.\r",
    ]);

    assert_eq!(
        updates,
        vec![
            GithubCloneProgress::new(GithubClonePhase::Counting, None),
            GithubCloneProgress::new(GithubClonePhase::Counting, Some(100)),
            GithubCloneProgress::new(GithubClonePhase::Compressing, Some(100)),
            GithubCloneProgress::new(GithubClonePhase::Receiving, Some(50)),
            GithubCloneProgress::new(GithubClonePhase::Receiving, Some(100)),
            GithubCloneProgress::new(GithubClonePhase::Resolving, Some(100)),
            GithubCloneProgress::new(GithubClonePhase::Checkout, Some(100)),
        ]
    );
}

#[test]
fn keeps_progress_monotonic_when_git_prints_late_or_repeated_phases() {
    let updates = collect(&[
        b"Receiving objects: 100% (12/12), done.\r",
        b"Resolving deltas: 100% (3/3), done.\r",
        b"Counting objects: 10% (1/12)\r",
        b"Receiving objects: 100% (12/12), done.\r",
    ]);

    assert_eq!(
        updates,
        vec![
            GithubCloneProgress::new(GithubClonePhase::Receiving, Some(100)),
            GithubCloneProgress::new(GithubClonePhase::Resolving, Some(100)),
        ]
    );
}

#[test]
fn merges_progress_lines_split_across_stderr_chunks() {
    let updates = collect(&[
        b"Receiving obj",
        b"ects:  4",
        b"2% (5/12), 2.00 MiB",
        b"\rResolving deltas: 100% (3/3), done.\r",
    ]);

    assert_eq!(
        updates,
        vec![
            GithubCloneProgress::new(GithubClonePhase::Receiving, Some(42)),
            GithubCloneProgress::new(GithubClonePhase::Resolving, Some(100)),
        ]
    );
}

#[test]
fn ignores_unrelated_stderr_and_rejects_out_of_range_percentages() {
    let updates = collect(&[
        b"fatal: could not read Password for 'https://github.com'\n",
        b"Receiving objects: 200% (13/12)\r",
        b"Receiving objects: 100% (12/12), done.\r",
    ]);

    assert_eq!(
        updates,
        vec![GithubCloneProgress::new(
            GithubClonePhase::Receiving,
            Some(100)
        )]
    );
}
