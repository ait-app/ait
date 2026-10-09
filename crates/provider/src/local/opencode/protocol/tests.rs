#[test]
fn shared_lifecycle_and_projection_do_not_branch_on_wire_versions() {
    for (path, source) in [
        ("session", include_str!("../session.rs")),
        ("live", include_str!("../live.rs")),
        ("approvals", include_str!("../approvals.rs")),
        ("bridge", include_str!("../bridge.rs")),
        ("publication", include_str!("../publication.rs")),
        ("summary", include_str!("../summary.rs")),
        ("discovery", include_str!("../client/native_sessions.rs")),
    ] {
        for native_detail in [
            "Version::",
            "api.version",
            "\"/abort\"",
            "\"/interrupt\"",
            "\"modelID\"",
        ] {
            assert!(
                !source.contains(native_detail),
                "{path} leaked native detail {native_detail}"
            );
        }
    }
}
