//! Path relevance rules and recursive-watch coverage for checkout invalidation.

use std::path::{Path, PathBuf};

use super::{WatchRoots, cover};
use crate::git::local::checkout::summary::GitDirs;
use std::collections::BTreeSet;

fn linked() -> WatchRoots {
    WatchRoots::new(
        Path::new("/work/feature/sub"),
        Some(GitDirs {
            work_tree: PathBuf::from("/work/feature"),
            git_dir: PathBuf::from("/repo/.git/worktrees/feature"),
            common_dir: PathBuf::from("/repo/.git"),
        }),
    )
}

fn main_checkout() -> WatchRoots {
    WatchRoots::new(
        Path::new("/repo"),
        Some(GitDirs {
            work_tree: PathBuf::from("/repo"),
            git_dir: PathBuf::from("/repo/.git"),
            common_dir: PathBuf::from("/repo/.git"),
        }),
    )
}

#[test]
fn linked_worktrees_follow_their_files_private_state_and_shared_references() {
    let roots = linked();
    for relevant in [
        "/work/feature",
        "/work/feature/src/main.rs",
        "/repo/.git/worktrees/feature/index",
        "/repo/.git/worktrees/feature/HEAD",
        "/repo/.git/refs/remotes/origin/main",
        "/repo/.git/packed-refs",
        "/repo/.git/config",
        "/repo/.git/info/exclude",
    ] {
        assert!(roots.affects(Path::new(relevant)), "{relevant}");
    }
    for unrelated in [
        "/work/feature-other/file",
        "/work/featurex",
        "/repo/.git/worktrees/other/index",
        "/repo/.git/worktrees/feature/logs/HEAD",
        "/repo/.git/objects/ab/cdef",
        "/repo/.git/index",
        "/repo/.git/HEAD",
        "/repo/src/main.rs",
    ] {
        assert!(!roots.affects(Path::new(unrelated)), "{unrelated}");
    }
}

#[test]
fn main_checkouts_follow_their_own_git_state_but_not_linked_worktrees() {
    let roots = main_checkout();
    for relevant in ["/repo/src/lib.rs", "/repo/.git/index", "/repo/.git/HEAD"] {
        assert!(roots.affects(Path::new(relevant)), "{relevant}");
    }
    for unrelated in [
        "/repo/.git/worktrees/feature/index",
        "/repo/.git/objects/pack/pack-1.pack",
        "/repo/.git/logs/HEAD",
    ] {
        assert!(!roots.affects(Path::new(unrelated)), "{unrelated}");
    }
    let plain = WatchRoots::new(Path::new("/plain"), None);
    assert!(plain.affects(Path::new("/plain/.git/HEAD")));
    assert!(!plain.affects(Path::new("/plainer")));
}

#[test]
fn checkouts_count_as_watched_only_when_every_root_is_covered() {
    let roots = linked();
    let mut watched = BTreeSet::from([PathBuf::from("/work")]);
    assert!(
        !roots.covered_by(&watched),
        "shared Git directory is not watched"
    );
    watched.insert(PathBuf::from("/repo"));
    assert!(roots.covered_by(&watched));
}

#[test]
fn recursive_watch_cover_drops_nested_and_duplicate_roots() {
    let linked = linked();
    let main = main_checkout();
    let covered = cover(linked.paths().chain(main.paths()));
    assert_eq!(
        covered.into_iter().collect::<Vec<_>>(),
        [PathBuf::from("/repo"), PathBuf::from("/work/feature")]
    );
}
