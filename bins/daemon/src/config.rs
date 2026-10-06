use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use secrecy::SecretString;
use serde::Deserialize;

#[derive(Debug, Parser)]
#[command(name = "daemon", version, about = "Ait local daemon")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Daemon data directory (default: `AIT_SERVER_DATA_DIR` or ~/.ait-server).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    /// IP socket address (default: `AIT_SERVER_LISTEN`, config, or 127.0.0.1:7316).
    #[arg(long, global = true)]
    listen: Option<SocketAddr>,
    /// Non-secret TOML configuration (default: <data-dir>/config.toml, if present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Logging threshold: error, warn, info, debug, trace, or off.
    #[arg(long, global = true)]
    log_level: Option<String>,
    /// Allowed browser page origin; repeat for multiple local frontends.
    #[arg(long, global = true)]
    web_origin: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Authorize this Linux daemon through Web approval or a single-use enrollment token.
    Login(Login),
    /// Revoke the machine grant and clear private local credentials (stop the service first).
    Logout,
    /// Run the existing daemon, optionally with independent center-managed Host publication.
    Run {
        /// Own authorization, lease renewal and Relay without a desktop client.
        #[arg(long)]
        headless: bool,
    },
    /// Read non-secret authorization and runtime status.
    Status {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub(super) struct Login {
    /// Host display label; defaults to the machine hostname.
    #[arg(long)]
    pub name: Option<String>,
    /// Single-use enrollment JWT from the Host page (prefer stdin for scripts).
    #[arg(long, conflicts_with = "token_stdin", value_parser = parse_secret)]
    pub token: Option<SecretString>,
    /// Read enrollment JWT from stdin; empty/invalid input never falls back to Web login.
    #[arg(long, conflicts_with = "token")]
    pub token_stdin: bool,
}

fn parse_secret(value: &str) -> Result<SecretString, String> {
    if value.is_empty() {
        return Err("enrollment token must not be empty".to_owned());
    }
    Ok(value.to_owned().into())
}

pub(super) fn data_directory(
    cli: &Cli,
    env: impl Fn(&str) -> Option<OsString>,
) -> anyhow::Result<PathBuf> {
    let path = cli
        .data_dir
        .clone()
        .or_else(|| env("AIT_SERVER_DATA_DIR").map(PathBuf::from))
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".ait-server")))
        .context("set --data-dir or AIT_SERVER_DATA_DIR when HOME is unavailable")?;
    if path.as_os_str().is_empty() {
        bail!("data directory must not be empty");
    }
    Ok(path)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    listen: Option<SocketAddr>,
    log_level: Option<String>,
    web_origins: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(super) struct Config {
    pub data_dir: PathBuf,
    pub listen: SocketAddr,
    pub token: SecretString,
    pub log_level: tracing::level_filters::LevelFilter,
    pub web_origins: Vec<String>,
    pub headless: bool,
}

impl Config {
    pub fn load(cli: Cli, env: impl Fn(&str) -> Option<OsString>) -> anyhow::Result<Self> {
        let headless = matches!(cli.command, Some(Command::Run { headless: true }));
        if headless && !cfg!(target_os = "linux") {
            bail!("headless runtime currently requires Linux");
        }
        // Validate credentials before touching disk or opening the listener.
        let token = env("AIT_SERVER_TOKEN")
            .context("set AIT_SERVER_TOKEN before starting daemon")?
            .into_string()
            .map_err(|_| anyhow::anyhow!("AIT_SERVER_TOKEN must be UTF-8"))?;
        api::validate_token(&token)?;
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
        let file = match std::fs::read_to_string(&config_path) {
            Ok(text) => {
                // TOML diagnostics echo source lines, which may contain misplaced credentials.
                toml::from_str::<FileConfig>(&text).map_err(|_| {
                    anyhow::anyhow!(
                        "invalid daemon config; only listen, log_level and web_origins are supported"
                    )
                })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !explicit_config => {
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
            api::validate_browser_origin(origin)?;
        }
        Ok(Self {
            data_dir,
            listen,
            token,
            log_level,
            web_origins,
            headless,
        })
    }
}

#[cfg(test)]
mod tests;
