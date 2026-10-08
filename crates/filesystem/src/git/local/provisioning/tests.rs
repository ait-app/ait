use std::process::Command;

use super::*;

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn inspects_non_git_nested_git_and_linked_worktree_directories() {
    let fixture = tempfile::tempdir().unwrap();
    let plain = fixture.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let source = LocalDirectorySource;
    let checkout = source.inspect(plain.to_str().unwrap()).unwrap();
    assert!(!checkout.is_git);
    assert_eq!(checkout.cwd, plain.to_str().unwrap());

    let repository = fixture.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Server Test",
            "-c",
            "user.email=server@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "baseline",
        ],
    );
    let nested = repository.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let checkout = source.inspect(nested.to_str().unwrap()).unwrap();
    assert_eq!(checkout.cwd, nested.to_str().unwrap());
    assert_eq!(checkout.current_branch.as_deref(), Some("main"));
    assert!(source.equivalent(
        checkout.worktree_root.as_deref().unwrap(),
        repository.to_str().unwrap()
    ));
    assert!(checkout.main_repo_root.is_none());

    let linked = fixture.path().join("linked");
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let checkout = source.inspect(linked.to_str().unwrap()).unwrap();
    assert!(checkout.is_git);
    assert!(checkout.current_branch.is_none());
    assert!(source.equivalent(
        checkout.main_repo_root.as_deref().unwrap(),
        repository.to_str().unwrap()
    ));
}

#[test]
fn creates_one_child_and_compares_realpath_aliases() {
    let fixture = tempfile::tempdir().unwrap();
    let source = LocalDirectorySource;
    let created = source
        .create_child(fixture.path().to_str().unwrap(), "project")
        .unwrap();
    assert!(Path::new(&created).is_dir());
    assert_eq!(
        source.create_child(fixture.path().to_str().unwrap(), "project"),
        Err(DirectorySourceError::AlreadyExists)
    );

    #[cfg(unix)]
    {
        let alias = fixture.path().join("alias");
        std::os::unix::fs::symlink(&created, &alias).unwrap();
        assert!(source.equivalent(&created, alias.to_str().unwrap()));
    }
    source.remove_empty(&created).unwrap();
    assert!(!Path::new(&created).exists());
}

#[test]
fn missing_and_non_directory_paths_are_rejected() {
    let fixture = tempfile::tempdir().unwrap();
    let file = fixture.path().join("file");
    std::fs::write(&file, "content").unwrap();
    let source = LocalDirectorySource;
    assert_eq!(
        source.inspect(file.to_str().unwrap()),
        Err(DirectorySourceError::NotFound)
    );
    assert_eq!(
        source.inspect(fixture.path().join("missing").to_str().unwrap()),
        Err(DirectorySourceError::NotFound)
    );
}

#[test]
fn path_aliases_are_normalized_before_inspection_and_file_parents_are_rejected() {
    let source = LocalDirectorySource;
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(
        source.canonical(".").unwrap(),
        cwd.canonicalize().unwrap().to_str().unwrap()
    );
    let home = std::env::var("HOME").unwrap();
    assert_eq!(
        source.canonical("~").unwrap(),
        source.canonical(&home).unwrap()
    );
    assert_eq!(
        source.canonical("~/.").unwrap(),
        source.canonical(&home).unwrap()
    );
    assert!(source.equivalent("missing/../future", "./future"));
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "occupied").unwrap();
    assert_eq!(
        source.create_child(file.to_str().unwrap(), "child"),
        Err(DirectorySourceError::NotFound)
    );
    assert_eq!(
        source.canonical(file.to_str().unwrap()),
        Err(DirectorySourceError::NotFound)
    );
    assert_eq!(
        source.remove_empty(file.to_str().unwrap()),
        Err(DirectorySourceError::NotFound)
    );
    assert_eq!(std::fs::read_to_string(file).unwrap(), "occupied");
}
