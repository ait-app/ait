use super::UsageIdentity;

#[test]
fn identities_separate_providers_and_never_format_authentication_material() {
    let identity = UsageIdentity::from_native("codex", b"fixture-credential");
    assert_eq!(
        identity,
        UsageIdentity::from_native("codex", b"fixture-credential")
    );
    assert_ne!(
        identity,
        UsageIdentity::from_native("claude", b"fixture-credential")
    );
    assert_ne!(
        identity,
        UsageIdentity::from_native("codex", b"rotated-credential")
    );
    assert_eq!(format!("{identity:?}"), "UsageIdentity(<redacted>)");
}
