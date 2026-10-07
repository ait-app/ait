use super::*;

fn load(cli: Cli, env: impl Fn(&str) -> Option<OsString>) -> anyhow::Result<Config> {
    Config::load(
        cli,
        env,
        |token| {
            anyhow::ensure!(token.len() >= 32, "invalid test credential");
            Ok(())
        },
        |origin| {
            anyhow::ensure!(
                ["http://localhost:", "http://127.0.0.1:", "http://[::1]:"]
                    .iter()
                    .any(|prefix| origin.starts_with(prefix)),
                "invalid test origin"
            );
            Ok(())
        },
    )
}

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
    let config = load(Cli::parse_from(["server"]), environment).unwrap();
    assert_eq!(
        config.data_dir,
        PathBuf::from("/unused-home-for-config-test/.ait-server")
    );
    assert_eq!(config.listen, "127.0.0.1:7316".parse().unwrap());
    assert!(config.web_origins.is_empty());
    assert!(!format!("{config:?}").contains(TOKEN));
    assert!(load(Cli::parse_from(["server"]), |_| None).is_err());
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
        load(Cli::parse_from(args), environment)
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
        load(cli, environment).unwrap().web_origins,
        ["http://127.0.0.1:8082", "http://[::1]:8082"]
    );
    let cli = Cli::parse_from(args.into_iter().chain(["--web-origin", "http://evil.test"]));
    assert!(load(cli, environment).is_err());
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
    let file = load(Cli::parse_from(args), environment).unwrap();
    assert_eq!(file.listen.port(), 7001);
    assert_eq!(file.log_level, tracing::level_filters::LevelFilter::WARN);
    let env = |name: &str| match name {
        "AIT_SERVER_LISTEN" => Some("127.0.0.1:7002".into()),
        "AIT_SERVER_LOG_LEVEL" => Some("debug".into()),
        "AIT_SERVER_DATA_DIR" => Some(directory.path().as_os_str().to_owned()),
        _ => environment(name),
    };
    let config = load(Cli::parse_from(["server"]), env).unwrap();
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
    let cli = load(Cli::parse_from(args), env).unwrap();
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
        assert!(load(Cli::parse_from(args), environment).is_err());
    }
    assert!(Cli::try_parse_from(["server", "--data-dir", ""]).is_err());
    assert!(
        load(Cli::parse_from(["server"]), |name| {
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
            load(
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
    let error = load(
        Cli::parse_from(["server", "--data-dir", directory.path().to_str().unwrap()]),
        environment,
    )
    .unwrap_err();
    assert!(!format!("{error:#}").contains(TOKEN));
    assert!(
        load(Cli::parse_from(["server"]), |name| {
            if name == "AIT_SERVER_TOKEN" {
                Some("short".into())
            } else {
                environment(name)
            }
        })
        .is_err()
    );
    assert!(
        load(Cli::parse_from(["server"]), |name| {
            if name == "HOME" {
                None
            } else {
                environment(name)
            }
        })
        .is_err()
    );
    assert!(
        load(Cli::parse_from(["server"]), |name| {
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
        let config = load(Cli::parse_from(["server", "--listen", listen]), environment).unwrap();
        assert_eq!(config.listen, listen.parse().unwrap());
    }
}

#[test]
fn host_validation_runs_before_file_io_and_uses_the_resolved_origins() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "invalid toml").unwrap();
    let args = ["daemon", "--config", path.to_str().unwrap()];
    let error = Config::load(
        Cli::parse_from(args),
        environment,
        |token| {
            assert_eq!(token, TOKEN);
            anyhow::bail!("host rejected credential")
        },
        |_| panic!("origin validation must not run after a rejected credential"),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "host rejected credential");

    std::fs::write(&path, "web_origins = ['http://localhost:8081']").unwrap();
    let origins = std::cell::RefCell::new(Vec::new());
    let config = Config::load(
        Cli::parse_from(
            args.into_iter()
                .chain(["--web-origin", "http://127.0.0.1:8082"]),
        ),
        environment,
        |_| Ok(()),
        |origin| {
            origins.borrow_mut().push(origin.to_owned());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*origins.borrow(), config.web_origins);
    assert_eq!(config.web_origins, ["http://127.0.0.1:8082"]);
}
