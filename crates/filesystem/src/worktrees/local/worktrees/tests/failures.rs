use super::*;

#[test]
fn default_base_prefers_remote_head_then_current_branch_then_detached_fallback() {
    let fixture = Fixture::new();
    let repo = &fixture.repository;
    run(repo, &["branch", "release"]);
    run(repo, &["update-ref", "refs/remotes/origin/release", "HEAD"]);
    run(
        repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/release",
        ],
    );
    let adapter = fixture.adapter();
    for (slug, expected) in [
        ("remote-default", "refs/heads/release"),
        ("current-default", "refs/heads/main"),
        ("detached-main", "refs/heads/main"),
        ("detached-master", "refs/heads/master"),
    ] {
        let created = adapter
            .create(&ManagedWorktreeCreate {
                cwd: path(repo),
                slug: slug.to_owned(),
                mode: WorktreeCreateMode::BranchOff {
                    base_ref: None,
                    branch_name: slug.to_owned(),
                },
            })
            .unwrap();
        assert_eq!(created.comparison_base_ref.as_deref(), Some(expected));
        match slug {
            "remote-default" => run(
                repo,
                &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
            ),
            "current-default" => run(repo, &["switch", "--detach", "--quiet"]),
            "detached-main" => run(repo, &["branch", "-m", "main", "master"]),
            _ => run(repo, &["branch", "-m", "master", "trunk"]),
        }
    }
    let error = adapter
        .create(&ManagedWorktreeCreate {
            cwd: path(repo),
            slug: "unknown-default".to_owned(),
            mode: WorktreeCreateMode::BranchOff {
                base_ref: None,
                branch_name: "unknown-default".to_owned(),
            },
        })
        .unwrap_err();
    assert_eq!(
        error,
        WorktreeError::Io("Unable to resolve repository default branch".to_owned())
    );
    assert_eq!(adapter.list(&path(repo)).unwrap().len(), 4);
}

#[test]
fn invalid_sources_and_managed_roots_fail_without_creating_branches() {
    let fixture = Fixture::new();
    let mut request = ManagedWorktreeCreate {
        cwd: path(&fixture.repository),
        slug: "rejected".to_owned(),
        mode: WorktreeCreateMode::BranchOff {
            base_ref: Some("HEAD".to_owned()),
            branch_name: "rejected".to_owned(),
        },
    };
    assert!(matches!(
        fixture.adapter().create(&request),
        Err(WorktreeError::Invalid(_))
    ));
    request.mode = WorktreeCreateMode::BranchOff {
        base_ref: Some("main".to_owned()),
        branch_name: "rejected".to_owned(),
    };
    let occupied = fixture.root.path().join("occupied");
    std::fs::write(&occupied, "occupied").unwrap();
    assert!(matches!(
        managed(occupied).create(&request),
        Err(WorktreeError::Io(_))
    ));
    assert!(!local_branch_exists(&fixture.repository, "rejected"));
    for source in [
        fixture.root.path().join("missing"),
        fixture.repository.join("README.md"),
        fixture.root.path().to_path_buf(),
    ] {
        assert_eq!(
            fixture.adapter().list(&path(&source)),
            Err(WorktreeError::NotGitRepository)
        );
    }
}

#[test]
fn git_output_budget_is_enforced_even_when_the_process_has_already_exited() {
    use std::io::Write;
    let mut output = tempfile::tempfile().unwrap();
    output
        .write_all(&vec![b'x'; usize::try_from(OUTPUT_LIMIT).unwrap()])
        .unwrap();
    assert_eq!(read_output(&mut output).unwrap().len() as u64, OUTPUT_LIMIT);
    output.write_all(b"x").unwrap();
    assert_eq!(
        read_output(&mut output).unwrap_err(),
        WorktreeError::Io("Git command output exceeded limit".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn managed_storage_rejects_a_symbolic_link_without_writing_through_it() {
    let fixture = Fixture::new();
    let outside = fixture.root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &fixture.managed_root).unwrap();
    let result = fixture.adapter().create(&ManagedWorktreeCreate {
        cwd: path(&fixture.repository),
        slug: "safe".to_owned(),
        mode: WorktreeCreateMode::BranchOff {
            base_ref: Some("main".to_owned()),
            branch_name: "safe".to_owned(),
        },
    });
    assert_eq!(
        result,
        Err(WorktreeError::Io(
            "managed worktree root is not a directory".to_owned()
        ))
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    assert!(!local_branch_exists(&fixture.repository, "safe"));
}
