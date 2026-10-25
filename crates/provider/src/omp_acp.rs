//! Ait-owned ACP stdio adapter for the official Oh My Pi RPC runtime.

use std::path::PathBuf;

/// Serve ACP on stdin/stdout while one unmodified OMP process owns execution and plugins.
///
/// `program` is the installed OMP executable. Authentication and native configuration remain
/// with OMP. Protocol output is JSONL; diagnostics never go to stdout.
///
/// # Errors
/// Returns bounded protocol, native startup, or pipe failures. Closing stdin releases the
/// owned process and preserves its native history. User input is never automatically replayed.
pub async fn serve(program: PathBuf) -> std::io::Result<()> {
    crate::local::omp::serve_acp(program, tokio::io::stdin(), tokio::io::stdout()).await
}
