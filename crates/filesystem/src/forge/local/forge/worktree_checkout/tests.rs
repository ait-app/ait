use super::*;

#[test]
fn fork_checkout_facts_preserve_case_normalize_owner_and_require_automation_trust() {
    let target = parse_target(&serde_json::json!({"headRefName":"Feature/Test", "baseRefName":"main",
        "isCrossRepository":true,"headRepositoryOwner":{"login":"ForkOwner"},
        "headRepository":{"sshUrl":"git@github.com:ForkOwner/repository.git","url":"https://github.com/ForkOwner/repository"}}), 123, None).unwrap();
    assert_eq!(target.local_branch, "forkowner/Feature/Test");
    assert_eq!(target.head_ref, "Feature/Test");
    assert_eq!(
        target.untrusted_repository.as_deref(),
        Some("ForkOwner/repository")
    );
    assert!(!target.track_origin);
    assert_eq!(
        target.push_remote_url.as_deref(),
        Some("git@github.com:ForkOwner/repository.git")
    );
}

#[test]
fn same_repository_checkout_tracks_origin_and_an_override_selects_the_head_name() {
    let target = parse_target(
        &serde_json::json!({"headRefName":"old", "baseRefName":"main","isCrossRepository":false}),
        1,
        Some(" override "),
    )
    .unwrap();
    assert_eq!(target.local_branch, "override");
    assert!(target.track_origin);
    assert!(target.untrusted_repository.is_none());
    assert!(target.push_remote_url.is_none());
    assert!(parse_target(&serde_json::json!({"isCrossRepository":true,"headRepository":{"sshUrl":"ext::malicious"}}),1,None).is_err());
}
