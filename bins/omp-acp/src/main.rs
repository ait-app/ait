//! Standalone ACP stdio host for an independently installed OMP executable.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[cfg(test)]
use serde_json as _;
#[cfg(test)]
use tempfile as _;

#[derive(Debug, Parser)]
#[command(name = "ait-omp-acp", bin_name = "ait-omp-acp", version = model::BUILD_VERSION,
    about = "Serve ACP over stdio using an installed OMP runtime")]
struct Cli {
    /// Installed OMP executable (default: `AIT_SERVER_OMP_BIN`, then `omp` on PATH).
    #[arg(long)]
    omp_bin: Option<PathBuf>,
}

impl Cli {
    fn program(self, environment: impl FnOnce() -> Option<OsString>) -> PathBuf {
        self.omp_bin
            .or_else(|| environment().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("omp"))
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    let program = cli.program(|| std::env::var_os("AIT_SERVER_OMP_BIN"));
    match provider::omp_acp::serve(program, tokio::io::stdin(), tokio::io::stdout()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "OMP ACP bridge stopped");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
