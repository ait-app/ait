use super::bot_mode_override;

#[test]
fn native_automatic_review_is_disabled_without_changing_existing_tool_grants() {
    assert_eq!(
        bot_mode_override("codex", Some("auto-review")),
        Some("auto")
    );
    assert_eq!(bot_mode_override("claude", Some("auto")), Some("default"));
    for (provider, mode) in [
        ("codex", None),
        ("codex", Some("auto")),
        ("claude", Some("acceptEdits")),
        ("opencode", Some("build")),
        ("deepseek", Some("auto")),
    ] {
        assert_eq!(bot_mode_override(provider, mode), None);
    }
}
