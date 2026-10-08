use super::*;

#[test]
fn only_known_cloud_hosts_bypass_platform_probes() {
    assert_eq!(cloud_forge("github.com"), Some(ForgeKind::Github));
    assert_eq!(cloud_forge("team.ghe.com"), Some(ForgeKind::Github));
    assert_eq!(cloud_forge("gitlab.com"), Some(ForgeKind::Gitlab));
    for host in [
        "code.company.test",
        "github.com.evil.test",
        "gitlab.internal",
        "gitea.com",
    ] {
        assert_eq!(cloud_forge(host), None);
    }
}

#[test]
fn probe_hosts_cannot_be_options_or_contain_credentials() {
    for host in ["github.com", "gitlab-work", "code.corp"] {
        assert!(valid_host(host));
    }
    for host in [
        "",
        "-oProxyCommand=cmd",
        "name@host",
        "host/path",
        "host\nother",
        "host:443",
    ] {
        assert!(!valid_host(host));
    }
}
