//! Local daemon entry point.

mod config;
mod device;
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
        return if voice::offline::worker::run().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let cli = config::Cli::parse();
    let mut collector = None;
    match Box::pin(run(cli, &mut collector)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .try_init();
            tracing::error!(error = %format!("{error:#}"), "daemon stopped");
            if let Some((diagnostics, _instance)) = collector {
                diagnostics.flush(FLUSH_TIMEOUT);
            }
            ExitCode::FAILURE
        }
    }
}

/// How long a failing daemon waits for its final incident to be saved before exiting.
const FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Run one CLI command. Once the daemon's evidence collector starts it is stored in `collector`,
/// so the caller can save the incident for a fatal error before the process exits.
async fn run(
    mut cli: config::Cli,
    collector: &mut Option<(
        diagnostics::Diagnostics,
        std::sync::Arc<instance::InstanceLease>,
    )>,
) -> anyhow::Result<()> {
    if let Some(config::Command::OmpAcp { omp_bin }) = cli
        .command
        .take_if(|command| matches!(command, config::Command::OmpAcp { .. }))
    {
        let program = omp_bin
            .or_else(|| std::env::var_os("AIT_SERVER_OMP_BIN").map(Into::into))
            .unwrap_or_else(|| "omp".into());
        return provider::omp_acp::serve(program)
            .await
            .context("serve OMP ACP bridge");
    }
    let directory = config::data_directory(&cli, |name| std::env::var_os(name))?;
    match cli.command.take() {
        Some(config::Command::Login(options)) => return device::login(&directory, options).await,
        Some(config::Command::Logout) => return device::logout(&directory).await,
        Some(config::Command::Status { json }) => return device::status(&directory, json),
        Some(config::Command::OmpAcp { .. }) => {
            unreachable!("OMP ACP handled before daemon configuration")
        }
        command @ (Some(config::Command::Run) | None) => cli.command = command,
    }

    let (config, startup) = tokio::task::spawn_blocking(move || {
        config::Config::load_guarded(
            cli,
            |name| std::env::var_os(name),
            |token| api::validate_token(token).map_err(Into::into),
            |origin| api::validate_browser_origin(origin).map_err(Into::into),
            host::Startup::bind,
        )
    })
    .await
    .context("join daemon startup preparation")?
    .context("load daemon configuration")?;
    let diagnostics = diagnostics::Diagnostics::start(config.data_dir.join("diagnostics"))
        .context("start diagnostic collector")?;
    *collector = Some((diagnostics.clone(), startup.lease()));
    tracing_subscriber::registry()
        .with(diagnostics.clone().filtered())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::io::stderr)
                .with_filter(config.log_level),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize logging: {error}"))?;
    // Report (and, when an installer is reachable, prepare) the private OMP runtime. A missing
    // runtime only degrades `/vibe`, so this never blocks startup.
    host::native_host::ensure_runtime(&config.data_dir);
    loop {
        let server = startup
            .server(
                config.clone(),
                Some(std::sync::Arc::new(diagnostics.clone())),
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
