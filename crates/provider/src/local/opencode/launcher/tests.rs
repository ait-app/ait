use super::supported;

#[test]
fn supported_releases_require_a_fixed_v2_catalog_and_preserve_v1() {
    for version in ["1.18.4", "2.0.26", "2.0.27", "2.1.0", "2.12.0"] {
        assert!(supported(version), "{version}");
    }
    for version in [
        "0.9.0", "2.0.0", "2.0.20", "2.0.25", "3.0.0", "2.0", "invalid",
    ] {
        assert!(!supported(version), "{version}");
    }
}
