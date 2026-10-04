use std::collections::HashMap;
use std::ffi::OsString;

use secrecy::{ExposeSecret, SecretString};

use super::{Config, ConfigError, endpoint};

const ID: &str = "rt_0123456789abcdef0123456789abcdef";
const TOKEN: &str = "fedcba9876543210fedcba9876543210";

fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let values: HashMap<String, OsString> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), OsString::from(*value)))
        .collect();
    move |name| values.get(name).cloned()
}

#[test]
fn origins_become_runtime_websocket_endpoints() {
    let cases = [
        (
            "https://bonsai.example.com",
            "wss://bonsai.example.com/runtime",
        ),
        (
            "https://bonsai.example.com/",
            "wss://bonsai.example.com/runtime",
        ),
        (
            "https://bonsai.example.com:8443",
            "wss://bonsai.example.com:8443/runtime",
        ),
        ("http://localhost:8860", "ws://localhost:8860/runtime"),
        ("http://LOCALHOST:8860", "ws://localhost:8860/runtime"),
        ("http://127.0.0.2:9000", "ws://127.0.0.2:9000/runtime"),
        ("http://[::1]:8860", "ws://[::1]:8860/runtime"),
        ("https://127.0.0.1", "wss://127.0.0.1/runtime"),
    ];
    for (origin, expected) in cases {
        let converted = endpoint(origin).expect("valid origin");
        assert_eq!(converted.as_str(), expected, "{origin}");
    }
}

#[test]
fn origins_with_extra_parts_are_rejected() {
    for origin in [
        "https://bonsai.example.com/runtime",
        "https://bonsai.example.com/mcp",
        "https://bonsai.example.com/?a=1",
        "https://bonsai.example.com/#x",
        "https://user@bonsai.example.com",
        "https://user:pass@bonsai.example.com",
        "wss://bonsai.example.com",
        "ftp://bonsai.example.com",
        "bonsai.example.com",
        "",
    ] {
        assert_eq!(endpoint(origin), Err(ConfigError::InvalidUrl), "{origin}");
    }
}

#[test]
fn plain_http_is_limited_to_loopback_hosts() {
    for origin in [
        "http://bonsai.example.com",
        "http://localhost.example.com",
        "http://128.0.0.1",
        "http://10.0.0.1:8860",
        "http://[::2]",
        "http://0.0.0.0",
    ] {
        assert_eq!(endpoint(origin), Err(ConfigError::InsecureUrl), "{origin}");
    }
}

#[test]
fn unset_variables_disable_the_adapter() {
    assert!(matches!(Config::from_env(lookup(&[])), Ok(None)));
}

#[test]
fn partial_variables_fail_startup() {
    for pairs in [
        vec![("BONSAI_RUNTIME_URL", "https://example.com")],
        vec![("BONSAI_RUNTIME_ID", ID), ("BONSAI_RUNTIME_TOKEN", TOKEN)],
        vec![
            ("BONSAI_RUNTIME_URL", "https://example.com"),
            ("BONSAI_RUNTIME_TOKEN", TOKEN),
        ],
    ] {
        assert_eq!(
            Config::from_env(lookup(&pairs)).err(),
            Some(ConfigError::Partial)
        );
    }
}

#[test]
fn complete_variables_produce_a_redacted_config() {
    let config = Config::from_env(lookup(&[
        ("BONSAI_RUNTIME_URL", "http://localhost:8860"),
        ("BONSAI_RUNTIME_ID", ID),
        ("BONSAI_RUNTIME_TOKEN", TOKEN),
    ]))
    .expect("valid variables")
    .expect("enabled");
    assert_eq!(config.endpoint().as_str(), "ws://localhost:8860/runtime");
    assert_eq!(config.runtime_id(), ID);
    assert_eq!(config.token().expose_secret(), TOKEN);
    let debug = format!("{config:?}");
    assert!(!debug.contains(TOKEN));
    assert!(debug.contains("[redacted]"));
}

#[test]
fn malformed_values_are_named_without_echoing_them() {
    let base = [
        ("BONSAI_RUNTIME_URL", "https://example.com"),
        ("BONSAI_RUNTIME_ID", ID),
        ("BONSAI_RUNTIME_TOKEN", TOKEN),
    ];
    let cases: [(&str, &str, ConfigError); 5] = [
        ("BONSAI_RUNTIME_ID", "rt_short", ConfigError::InvalidId),
        (
            "BONSAI_RUNTIME_ID",
            "0123456789abcdef0123456789abcdef",
            ConfigError::InvalidId,
        ),
        (
            "BONSAI_RUNTIME_TOKEN",
            "FEDCBA9876543210FEDCBA9876543210",
            ConfigError::InvalidToken,
        ),
        (
            "BONSAI_RUNTIME_TOKEN",
            " fedcba9876543210fedcba987654321",
            ConfigError::InvalidToken,
        ),
        (
            "BONSAI_RUNTIME_URL",
            "http://example.com",
            ConfigError::InsecureUrl,
        ),
    ];
    for (name, value, expected) in cases {
        let mut pairs: Vec<(&str, &str)> = base.to_vec();
        pairs.retain(|(key, _)| *key != name);
        pairs.push((name, value));
        let error = Config::from_env(lookup(&pairs)).err();
        assert_eq!(error, Some(expected), "{name}");
        assert!(!expected.to_string().contains(value));
    }
}

#[test]
fn direct_construction_validates_the_token() {
    let error = Config::new(
        "https://example.com",
        ID.to_owned(),
        SecretString::from("not-hex"),
    )
    .err();
    assert_eq!(error, Some(ConfigError::InvalidToken));
}

#[cfg(unix)]
#[test]
fn non_utf8_values_are_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let lookup =
        |name: &str| (name == "BONSAI_RUNTIME_URL").then(|| OsString::from_vec(vec![0xff, 0xfe]));
    assert_eq!(
        Config::from_env(lookup).err(),
        Some(ConfigError::NotUtf8("BONSAI_RUNTIME_URL"))
    );
}
