use super::*;

#[test]
fn classifies_supported_old_and_unverified_release_strings() {
    for old in ["1.3.0", "1.5.13", "1.5.14\n"] {
        let status = classify(old.as_bytes());
        assert_eq!(status, Status::Legacy(old.trim().to_owned()));
        assert!(!status.has_completion_fixes());
        let guidance = status.upgrade_guidance().unwrap();
        assert!(guidance.contains(old.trim()));
        assert!(guidance.contains("1.5.15"));
    }
    for bad in [
        "1.5.15-beta",
        "unknown",
        "",
        "agy 1.5.18",
        "1.5",
        "1.5.18.1",
    ] {
        let status = classify(bad.as_bytes());
        assert_eq!(status, Status::Unverified, "{bad}");
        assert!(!status.has_completion_fixes());
        assert!(status.upgrade_guidance().unwrap().contains("1.5.15"));
    }
    for ok in ["1.5.15", "1.5.18\n", "1.6.0", "2.0.0"] {
        let status = classify(ok.as_bytes());
        assert_eq!(status, Status::Supported(ok.trim().to_owned()), "{ok}");
        assert!(status.has_completion_fixes());
        assert_eq!(status.upgrade_guidance(), None);
    }
}
