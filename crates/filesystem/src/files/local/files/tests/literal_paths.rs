use super::*;

#[test]
fn renaming_untracked_metacharacters_does_not_match_tracked_siblings() {
    let (_temp, files, cwd) = fixture();
    let root = Path::new(&cwd);
    crate::support::git_command::run(root, &["init", "-b", "main"]).unwrap();
    fs::write(root.join("a.txt"), "tracked\n").unwrap();
    crate::support::git_command::run(root, &["add", "a.txt"]).unwrap();
    fs::write(root.join("[a].txt"), "untracked\n").unwrap();

    assert_eq!(
        files.rename(&cwd, "[a].txt", "renamed.txt").unwrap(),
        "renamed.txt"
    );
    assert_eq!(
        fs::read_to_string(root.join("renamed.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "tracked\n");
    assert_eq!(
        crate::support::git_command::run(root, &["ls-files"]).unwrap(),
        "a.txt"
    );
}

#[cfg(unix)]
#[test]
fn directory_links_within_the_workspace_are_listed_as_directories() {
    let (temp, files, cwd) = fixture();
    let root = Path::new(&cwd);
    fs::create_dir(root.join("folder")).unwrap();
    fs::write(root.join("folder/inside.txt"), "inside").unwrap();
    std::os::unix::fs::symlink("folder", root.join("alias")).unwrap();
    std::os::unix::fs::symlink(temp.path(), root.join("outside")).unwrap();

    let (_, entries) = files.list(&cwd, ".").unwrap();
    let alias = entries.iter().find(|entry| entry.name == "alias").unwrap();
    assert_eq!(alias.kind, EntryKind::Directory);
    assert!(!entries.iter().any(|entry| entry.name == "outside"));
    let (_, children) = files.list(&cwd, "alias").unwrap();
    assert_eq!(children[0].path, "alias/inside.txt");
}
