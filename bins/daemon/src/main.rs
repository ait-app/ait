//! Local daemon entry point.

mod config;
mod diagnostics;
mod host;
mod instance;

use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::prelude::*;

// Binary unit tests inherit dependencies used by integration tests.
#[cfg(test)]
use futures_util as _;
#[cfg(test)]
use tokio_tungstenite as _;

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--speech-worker")
    {
        return if voice::offline::run_worker().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let cli = config::Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .try_init();
            tracing::error!(error = %format!("{error:#}"), "daemon stopped");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: config::Cli) -> anyhow::Result<()> {
    let config = config::Config::load(
        cli,
        |name| std::env::var_os(name),
        |token| api::validate_token(token).map_err(Into::into),
        |origin| api::validate_browser_origin(origin).map_err(Into::into),
    )
    .context("load daemon configuration")?;
    let diagnostics = diagnostics::Diagnostics::start(config.data_dir.join("diagnostics"))
        .context("start diagnostic collector")?;
    tracing_subscriber::registry()
        .with(
            diagnostics
                .clone()
                .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::io::stderr)
                .with_filter(config.log_level),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize logging: {error}"))?;
    loop {
        let server = host::Server::bind_with_diagnostics(
            config.clone(),
            std::sync::Arc::new(diagnostics.clone()),
        )
        .await?;
        tracing::info!(listen = %server.address(), "daemon ready");
        match server.serve(shutdown_signal()?).await? {
            Some(model::LifecycleIntent::Restart { reason }) => {
                tracing::info!(%reason, "restarting daemon");
            }
            Some(model::LifecycleIntent::Shutdown) | None => return Ok(()),
        }
    }
}

fn shutdown_signal() -> anyhow::Result<impl Future<Output = ()>> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("register SIGTERM handler")?;
    Ok(async move {
        #[cfg(unix)]
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result { tracing::error!(%error, "read interrupt signal"); }
            },
            _ = terminate.recv() => {},
        }
        #[cfg(not(unix))]
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "read interrupt signal");
        }
    })
}
