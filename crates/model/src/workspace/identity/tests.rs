use super::*;

fn checkout() -> Checkout {
    Checkout {
        cwd: "/tmp/alpha/nested".to_owned(),
        is_git: true,
        current_branch: Some("main".to_owned()),
        remote_url: Some("git@github.com:Example/Repo.git".to_owned()),
        worktree_root: Some("/tmp/alpha".to_owned()),
        is_paseo_owned_worktree: false,
        main_repo_root: None,
    }
}

#[test]
fn project_keys_preserve_subdirectories_and_remote_port_identity() {
    let mut checkout = checkout();
    assert_eq!(
        derive_project_key(&checkout, "server"),
        "remote:github.com/example/repo#subdir:nested"
    );
    checkout.remote_url = Some("ssh://git@git.example.com:60443/team/repo.git".to_owned());
    assert_eq!(
        derive_project_key(&checkout, "server"),
        "remote:git.example.com:60443/team/repo#subdir:nested"
    );
    checkout.remote_url = None;
    assert_eq!(
        derive_project_key(&checkout, "server"),
        "host:server:/tmp/alpha/nested"
    );
    checkout.main_repo_root = Some("/original".to_owned());
    assert_eq!(
        derive_project_key(&checkout, "server"),
        "host:server:/original/nested"
    );
    assert_eq!(basename("/tmp/alpha/nested"), "nested");
    assert_eq!(basename("/"), "/");
}

#[test]
fn remote_parsing_normalizes_transports_and_rejects_invalid_encoded_paths() {
    let expected = parse_remote("https://github.com/owner/repo.git").unwrap();
    for remote in [
        "git@github.com:owner/repo.git",
        "ssh://git@github.com:22/owner/repo.git",
        "https://GITHUB.COM.:443/%6fwner/%72epo.git",
        "https://github.com/owner%2Frepo.git?ignored#ref",
    ] {
        assert_eq!(parse_remote(remote), Some(expected.clone()), "{remote}");
    }
    for path in ["%", "%2", "%GG", "%ff"] {
        assert!(parse_remote(&format!("https://github.com/owner/{path}")).is_none());
    }
    for remote in [
        "file:///repo",
        "https://-invalid/repo",
        "https://github.com/",
        "invalid",
    ] {
        assert!(parse_remote(remote).is_none(), "{remote}");
    }
}
