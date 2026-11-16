use super::*;
use serde_json::{Value, json};

fn request(mut extra: Value) -> CreateRequest {
    let mut value = json!({"config":{"provider":"codex","cwd":"/repo"},"initialPrompt":"review"});
    value
        .as_object_mut()
        .unwrap()
        .extend(std::mem::take(extra.as_object_mut().unwrap()));
    serde_json::from_value(value).unwrap()
}

#[test]
fn modern_and_legacy_worktree_options_translate_to_the_owner_port() {
    let branch = intent(&request(
        json!({"worktree":{"mode":"branch-off","newBranch":"feature/review","base":"develop"}}),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(branch.action, WorktreeAction::BranchOff);
    assert_eq!(branch.worktree_slug.as_deref(), Some("feature/review"));
    assert_eq!(branch.ref_name.as_deref(), Some("develop"));
    assert_eq!(branch.first_agent_prompt.as_deref(), Some("review"));
    let checkout = intent(&request(
        json!({"worktree":{"mode":"checkout-branch","branch":"existing"}}),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(checkout.action, WorktreeAction::Checkout);
    assert_eq!(checkout.ref_name.as_deref(), Some("existing"));
    let legacy = intent(&request(json!({"worktreeName":"legacy"})))
        .unwrap()
        .unwrap();
    assert_eq!(legacy.worktree_slug.as_deref(), Some("legacy"));
    let configured = intent(&request(json!({"git":{"createWorktree":true,"newBranchName":"new","worktreeSlug":"directory","baseBranch":"main","action":"checkout","refName":"existing"}}))).unwrap().unwrap();
    assert_eq!(configured.worktree_slug.as_deref(), Some("directory"));
    assert_eq!(configured.action, WorktreeAction::Checkout);
    assert_eq!(configured.ref_name.as_deref(), Some("existing"));
    assert_eq!(configured.base_branch.as_deref(), Some("main"));
    assert!(
        intent(&request(json!({"git":{},"worktreeName":"ignored"})))
            .unwrap()
            .is_none()
    );
    assert!(intent(&request(json!({}))).unwrap().is_none());
    for git in [
        json!({"createNewBranch":true,"newBranchName":"new"}),
        json!({"baseBranch":"existing"}),
    ] {
        assert!(intent(&request(json!({"git":git}))).unwrap().is_none());
    }
}

#[test]
fn invalid_conflicting_and_uninstalled_placements_fail_before_provisioning() {
    for extra in [
        json!({"worktree":{"mode":"branch-off","newBranch":""}}),
        json!({"worktree":{"mode":"branch-off","newBranch":"new","base":""}}),
        json!({"worktree":{"mode":"checkout-branch","branch":"bad\nbranch"}}),
        json!({"worktree":{"mode":"checkout-pr","prNumber":0}}),
        json!({"worktree":{"mode":"checkout-branch","branch":"existing"},"git":{}}),
        json!({"git":{"createNewBranch":true}}),
        json!({"git":{"createNewBranch":true,"newBranchName":"!!!"}}),
        json!({"git":{"githubPrNumber":0}}),
    ] {
        assert_eq!(intent(&request(extra)), Err(ErrorCode::InvalidMessage));
    }
    assert!(
        intent(&request(json!({"git":{"githubPrNumber":1}})))
            .unwrap()
            .is_none()
    );
}

#[test]
fn modern_and_legacy_pull_requests_preserve_the_explicit_checkout_source() {
    for extra in [
        json!({"worktree":{"mode":"checkout-pr","prNumber":123}}),
        json!({"git":{"createWorktree":true,"githubPrNumber":123}}),
        json!({"git":{"createWorktree":true,"githubPrNumber":999,
            "checkoutSource":{"kind":"change_request","forge":"github","number":123}}}),
    ] {
        let input = intent(&request(extra)).unwrap().unwrap();
        assert_eq!(input.checkout_source.unwrap().number, 123);
    }
}
