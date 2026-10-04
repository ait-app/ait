use super::*;

const TOKEN: &str = "offline-config-test-token-longer-than-32";

fn environment(name: &str) -> Option<OsString> {
    match name {
        "AIT_SERVER_TOKEN" => Some(TOKEN.into()),
        "HOME" => Some("/unused-home-for-config-test".into()),
        _ => None,
    }
}

#[test]
fn defaults_are_isolated_and_secrets_are_redacted() {
    let config = Config::load(Cli::parse_from(["server"]), environment).unwrap();
    assert_eq!(
        config.data_dir,
        PathBuf::from("/unused-home-for-config-test/.ait-server")
    );
    assert_eq!(config.listen, "127.0.0.1:7316".parse().unwrap());
    assert!(config.web_origins.is_empty());
    assert!(!format!("{config:?}").contains(TOKEN));
    assert!(Config::load(Cli::parse_from(["server"]), |_| None).is_err());
}

#[test]
fn browser_origins_are_opt_in_validated_and_cli_overrides_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "web_origins = ['http://localhost:8081']",
    )
    .unwrap();
    let args = ["server", "--data-dir", directory.path().to_str().unwrap()];
    assert_eq!(
        Config::load(Cli::parse_from(args), environment)
            .unwrap()
            .web_origins,
        ["http://localhost:8081"]
    );
    let cli = Cli::parse_from(args.into_iter().chain([
        "--web-origin",
        "http://127.0.0.1:8082",
        "--web-origin",
        "http://[::1]:8082",
    ]));
    assert_eq!(
        Config::load(cli, environment).unwrap().web_origins,
        ["http://127.0.0.1:8082", "http://[::1]:8082"]
    );
    let cli = Cli::parse_from(args.into_iter().chain(["--web-origin", "http://evil.test"]));
    assert!(Config::load(cli, environment).is_err());
}

#[test]
fn cli_overrides_environment_which_overrides_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "listen = '127.0.0.1:7001'\nlog_level = 'warn'\n",
    )
    .unwrap();
    let args = ["server", "--data-dir", directory.path().to_str().unwrap()];
    let file = Config::load(Cli::parse_from(args), environment).unwrap();
    assert_eq!(file.listen.port(), 7001);
    assert_eq!(file.log_level, tracing::level_filters::LevelFilter::WARN);
    let env = |name: &str| match name {
        "AIT_SERVER_LISTEN" => Some("127.0.0.1:7002".into()),
        "AIT_SERVER_LOG_LEVEL" => Some("debug".into()),
        "AIT_SERVER_DATA_DIR" => Some(directory.path().as_os_str().to_owned()),
        _ => environment(name),
    };
    let config = Config::load(Cli::parse_from(["server"]), env).unwrap();
    assert_eq!(config.listen.port(), 7002);
    assert_eq!(config.log_level, tracing::level_filters::LevelFilter::DEBUG);
    let args = [
        "server",
        "--data-dir",
        directory.path().to_str().unwrap(),
        "--listen",
        "[::1]:0",
        "--log-level",
        "error",
    ];
    let cli = Config::load(Cli::parse_from(args), env).unwrap();
    assert_eq!(cli.listen, "[::1]:0".parse().unwrap());
    assert_eq!(cli.log_level, tracing::level_filters::LevelFilter::ERROR);
}

#[test]
fn invalid_configuration_fails_without_creating_state() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("not-created");
    for args in [
        vec!["server", "--config", missing.to_str().unwrap()],
        vec!["server", "--log-level", "invalid"],
    ] {
        assert!(Config::load(Cli::parse_from(args), environment).is_err());
    }
    assert!(Cli::try_parse_from(["server", "--data-dir", ""]).is_err());
    assert!(
        Config::load(Cli::parse_from(["server"]), |name| {
            if name == "AIT_SERVER_DATA_DIR" {
                Some(OsString::new())
            } else {
                environment(name)
            }
        })
        .is_err()
    );
    assert!(!missing.exists());
    for contents in [
        "bad toml",
        "token = 'never-supported'",
        "listen = '127.0.0.1:65536'",
    ] {
        std::fs::write(directory.path().join("config.toml"), contents).unwrap();
        assert!(
            Config::load(
                Cli::parse_from(["server", "--data-dir", directory.path().to_str().unwrap()]),
                environment
            )
            .is_err()
        );
    }
    std::fs::write(
        directory.path().join("config.toml"),
        format!("token = '{TOKEN}'"),
    )
    .unwrap();
    let error = Config::load(
        Cli::parse_from(["server", "--data-dir", directory.path().to_str().unwrap()]),
        environment,
    )
    .unwrap_err();
    assert!(!format!("{error:#}").contains(TOKEN));
    assert!(
        Config::load(Cli::parse_from(["server"]), |name| {
            if name == "AIT_SERVER_TOKEN" {
                Some("short".into())
            } else {
                environment(name)
            }
        })
        .is_err()
    );
    assert!(
        Config::load(Cli::parse_from(["server"]), |name| {
            if name == "HOME" {
                None
            } else {
                environment(name)
            }
        })
        .is_err()
    );
    assert!(
        Config::load(Cli::parse_from(["server"]), |name| {
            if name == "AIT_SERVER_LISTEN" {
                Some("invalid".into())
            } else {
                environment(name)
            }
        })
        .is_err()
    );
}

#[test]
fn supports_explicit_network_listeners_without_changing_loopback_default() {
    for listen in ["0.0.0.0:7316", "[::]:0", "192.168.1.2:7316"] {
        let config =
            Config::load(Cli::parse_from(["server", "--listen", listen]), environment).unwrap();
        assert_eq!(config.listen, listen.parse().unwrap());
    }
}

const BONSAI_ID: &str = "rt_0123456789abcdef0123456789abcdef";
const BONSAI_TOKEN: &str = "fedcba9876543210fedcba9876543210";

fn with_bonsai(name: &str) -> Option<OsString> {
    match name {
        "BONSAI_RUNTIME_URL" => Some("http://localhost:8860".into()),
        "BONSAI_RUNTIME_ID" => Some(BONSAI_ID.into()),
        "BONSAI_RUNTIME_TOKEN" => Some(BONSAI_TOKEN.into()),
        other => environment(other),
    }
}

#[test]
fn bonsai_runtime_is_off_unless_configured_and_never_printed() {
    let config = Config::load(Cli::parse_from(["server"]), environment).unwrap();
    assert!(config.bonsai.is_none());
    let config = Config::load(Cli::parse_from(["server"]), with_bonsai).unwrap();
    let bonsai = config.bonsai.as_ref().expect("enabled");
    assert_eq!(bonsai.endpoint().as_str(), "ws://localhost:8860/runtime");
    assert!(!format!("{config:?}").contains(BONSAI_TOKEN));
}

#[test]
fn partial_bonsai_runtime_configuration_stops_startup_before_disk() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    let partial = |name: &str| match name {
        "BONSAI_RUNTIME_URL" => Some("https://bonsai.example.com".into()),
        other => environment(other),
    };
    let args = ["server", "--data-dir", state.to_str().unwrap()];
    let error = Config::load(Cli::parse_from(args), partial).unwrap_err();
    assert!(format!("{error:#}").contains("BONSAI_RUNTIME_TOKEN"));
    assert!(!state.exists());
    let insecure = |name: &str| match name {
        "BONSAI_RUNTIME_URL" => Some("http://bonsai.example.com".into()),
        other => with_bonsai(other),
    };
    assert!(Config::load(Cli::parse_from(["server"]), insecure).is_err());
}

#[test]
fn the_websocket_library_never_logs_its_handshake_at_trace() {
    use tracing::Level;
    let mut config = Config::load(Cli::parse_from(["server"]), environment).expect("config");
    config.log_level = tracing::level_filters::LevelFilter::TRACE;

    let filter = config.log_filter();

    assert!(!filter.would_enable("tungstenite::handshake::client", &Level::TRACE));
    assert!(!filter.would_enable("tokio_tungstenite", &Level::TRACE));
    assert!(filter.would_enable("tungstenite::handshake::client", &Level::DEBUG));
    assert!(filter.would_enable("bonsai::link", &Level::TRACE));
    config.log_level = tracing::level_filters::LevelFilter::INFO;
    assert!(
        !config
            .log_filter()
            .would_enable("tungstenite", &Level::DEBUG)
    );
}
