use super::*;

#[test]
fn creation_preserves_workspace_intent_and_rejects_conflicting_modes() {
    let caller = json!({"cwd":"/repo","workspaceId":"ws"});
    let params =
        creation::workspace(json!({"isolation":"local","title":"Task"}), Some(&caller)).unwrap();
    assert_eq!(
        params,
        json!({"source":{"kind":"directory","path":"/repo"},"title":"Task"})
    );
    for (mode, extra, expected) in [
        (
            "branch-off",
            json!({"branchName":"task"}),
            json!({"branchName":"task","action":"branch-off"}),
        ),
        (
            "checkout-branch",
            json!({"branch":"existing"}),
            json!({"refName":"existing","action":"checkout"}),
        ),
        (
            "checkout-pr",
            json!({"prNumber":12,"forge":"github"}),
            json!({"checkoutSource":{"kind":"change_request","forge":"github","number":12},"action":"checkout"}),
        ),
    ] {
        let mut input =
            json!({"isolation":"worktree","path":"/repo","mode":mode,"projectId":"project"});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let value = creation::workspace(input, None).unwrap();
        serde_json::from_value::<domain::workspace::protocol::directory::WorkspaceCreateSource>(
            value["source"].clone(),
        )
        .unwrap();
        for (key, expected) in expected.as_object().unwrap() {
            assert_eq!(&value["source"][key], expected);
        }
        assert_eq!(value["source"]["projectId"], "project");
    }
    for input in [
        json!({"isolation":"local"}),
        json!({"isolation":"local","path":"/repo","branch":"bad"}),
        json!({"isolation":"worktree","mode":"checkout-branch"}),
        json!({"isolation":"worktree","mode":"checkout-pr","branch":"bad","prNumber":12}),
    ] {
        assert!(creation::workspace(input, None).is_err());
    }
}

#[test]
fn provider_pairs_keep_slashes_inside_model_names() {
    assert_eq!(
        creation::provider_model(&json!("codex/openai/model"), true).unwrap(),
        ("codex", Some("openai/model"))
    );
    assert_eq!(
        creation::provider_model(&json!("codex"), false).unwrap(),
        ("codex", None)
    );
    for value in [json!("codex"), json!("/model"), json!("codex/"), json!("")] {
        assert!(creation::provider_model(&value, true).is_err());
    }
}

#[test]
fn key_tokens_translate_special_keys_and_preserve_literal_text() {
    for (token, text) in [
        ("Enter", "\r"),
        ("Tab", "\t"),
        ("Escape", "\u{1b}"),
        ("Space", " "),
        ("BSpace", "\u{7f}"),
        ("C-c", "\u{3}"),
        ("C-d", "\u{4}"),
        ("C-z", "\u{1a}"),
        ("C-l", "\u{c}"),
        ("C-a", "\u{1}"),
        ("C-e", "\u{5}"),
        ("hello", "hello"),
    ] {
        assert_eq!(key_token(token), text);
    }
}
