use super::*;

#[test]
fn center_urls_require_tls_and_preserve_proxy_prefix() {
    assert!(center_url("http://example.com").is_err());
    assert!(center_url("https://user:password@example.com").is_err());
    assert!(center_url("https://example.com?token=secret").is_err());
    assert!(center_url("file:///tmp/center").is_err());
    assert!(center_url("http://127.0.0.1:3000").is_ok());
    let center = center_url("https://example.com/api").unwrap();
    assert_eq!(endpoint(&center, "v1/hub"), "wss://example.com/api/v1/hub");
}

#[test]
fn wildcard_listeners_are_replaced_by_loopback_destinations() {
    for (listen, expected) in [
        ("0.0.0.0:7316", "ws://127.0.0.1:7316/v1/ws"),
        ("[::]:7316", "ws://[::1]:7316/v1/ws"),
    ] {
        let connector = Connector::new(listen.parse().unwrap(), "server".into(), "instance".into());
        assert_eq!(connector.local.url, expected);
    }
}
