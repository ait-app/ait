use super::*;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn immutable_reads_share_one_result_while_symbolic_refs_and_errors_are_not_cached() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.test"]);
    std::fs::write(root.join("old.txt"), "text\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["mv", "old.txt", "new.txt"]);
    git(root, &["commit", "-qm", "rename"]);
    let sha = git(root, &["rev-parse", "HEAD"]);
    let cache = RenameCache::default();
    let first = cache.read(root, &sha).unwrap();
    assert_eq!(first.get("new.txt").map(String::as_str), Some("old.txt"));
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| cache.read(root, &sha).unwrap()))
            .collect();
        for thread in threads {
            assert!(Arc::ptr_eq(&first, &thread.join().unwrap()));
        }
    });
    let entries = cache.entries.lock().unwrap().len();
    assert_eq!(*cache.read(root, "HEAD").unwrap(), *first);
    assert_eq!(cache.entries.lock().unwrap().len(), entries);
    assert!(cache.read(root, &"f".repeat(40)).is_err());
    assert_eq!(cache.entries.lock().unwrap().len(), entries);
    cache
        .entries
        .lock()
        .unwrap()
        .get_mut(&(root.to_path_buf(), sha.clone()))
        .unwrap()
        .created = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    assert!(!Arc::ptr_eq(&first, &cache.read(root, &sha).unwrap()));
}
