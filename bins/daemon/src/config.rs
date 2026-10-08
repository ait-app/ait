//! Daemon startup configuration: CLI/environment precedence, TOML loading, and host validation.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Parser;
use persistence::File;
use secrecy::SecretString;
use serde::Deserialize;

/// Daemon startup arguments; CLI values override environment and TOML settings.
#[derive(Debug, Parser)]
#[command(name = "daemon", version, about = "Ait local daemon")]
pub struct Cli {
    /// Daemon data directory (default: `AIT_SERVER_DATA_DIR` or ~/.ait-server).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// IP socket address (default: `AIT_SERVER_LISTEN`, config, or 127.0.0.1:7316).
    #[arg(long)]
    listen: Option<SocketAddr>,
    /// Non-secret TOML configuration (default: <data-dir>/config.toml, if present).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Logging threshold: error, warn, info, debug, trace, or off.
    #[arg(long)]
    log_level: Option<String>,
    /// Allowed browser page origin; repeat for multiple local frontends.
    #[arg(long)]
    web_origin: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    listen: Option<SocketAddr>,
    log_level: Option<String>,
    web_origins: Option<Vec<String>>,
}

/// Resolved startup settings with a redacted credential and validated browser origins.
#[derive(Debug, Clone)]
pub struct Config {
    /// Directory owned by the host's process lease.
    pub data_dir: PathBuf,
    /// Resolved socket address.
    pub listen: SocketAddr,
    /// Credential supplied through the environment, never the TOML file.
    pub token: SecretString,
    /// Resolved logging threshold.
    pub log_level: tracing::level_filters::LevelFilter,
    /// Browser origins accepted by the injected host policy.
    pub web_origins: Vec<String>,
}

impl Config {
    /// Resolve `cli`, values from `env`, and the selected TOML file into startup settings.
    /// `validate_token` runs before file I/O; `validate_origin` checks every resolved origin.
    /// Both policies are injected by the host, keeping transport dependencies outside this crate.
    /// # Errors
    /// Returns missing credentials, validation, file, TOML, address, or logging errors.
    pub fn load(
        cli: Cli,
        env: impl Fn(&str) -> Option<OsString>,
        validate_token: impl FnOnce(&str) -> anyhow::Result<()>,
        validate_origin: impl Fn(&str) -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        // Validate credentials before touching disk or opening the listener.
        let token = env("AIT_SERVER_TOKEN")
            .context("set AIT_SERVER_TOKEN before starting daemon")?
            .into_string()
            .map_err(|_| anyhow::anyhow!("AIT_SERVER_TOKEN must be UTF-8"))?;
        validate_token(&token)?;
        let token = SecretString::from(token);
        let data_dir = cli
            .data_dir
            .or_else(|| env("AIT_SERVER_DATA_DIR").map(PathBuf::from))
            .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".ait-server")))
            .context("set --data-dir or AIT_SERVER_DATA_DIR when HOME is unavailable")?;
        if data_dir.as_os_str().is_empty() {
            bail!("data directory must not be empty");
        }
        let explicit_config = cli.config.is_some();
        let config_path = cli.config.unwrap_or_else(|| data_dir.join("config.toml"));
        let file = match File::new(&config_path).read_text() {
            Ok(text) => {
                // TOML diagnostics echo source lines, which may contain misplaced credentials.
                toml::from_str::<FileConfig>(&text).map_err(|_| {
                    anyhow::anyhow!(
                        "invalid daemon config; only listen, log_level and web_origins are supported"
                    )
                })?
            }
            Err(persistence::Error::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound && !explicit_config =>
            {
                FileConfig::default()
            }
            Err(error) => return Err(error).context("read daemon config"),
        };
        let listen = if let Some(value) = cli.listen {
            value
        } else if let Some(value) = env("AIT_SERVER_LISTEN") {
            value
                .to_str()
                .context("AIT_SERVER_LISTEN must be UTF-8")?
                .parse()
                .context("parse AIT_SERVER_LISTEN")?
        } else {
            file.listen
                .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 7316)))
        };
        let log_level = cli
            .log_level
            .or_else(|| env("AIT_SERVER_LOG_LEVEL").map(|v| v.to_string_lossy().into_owned()))
            .or(file.log_level)
            .unwrap_or_else(|| "info".to_owned())
            .parse()
            .context("parse log level")?;
        let web_origins = if cli.web_origin.is_empty() {
            file.web_origins.unwrap_or_default()
        } else {
            cli.web_origin
        };
        for origin in &web_origins {
            validate_origin(origin)?;
        }
        Ok(Self {
            data_dir,
            listen,
            token,
            log_level,
            web_origins,
        })
    }
}

#[cfg(test)]
mod tests;
