//! Ait-owned ACP stdio adapter for the official Oh My Pi RPC runtime.

use std::path::PathBuf;

/// Serve ACP on caller-owned streams while one installed OMP process owns execution and plugins.
///
/// `program` is the installed OMP executable; `input` and `output` carry ACP JSONL.
/// Authentication and native configuration remain with OMP. The caller owns process
/// arguments, stdio selection and diagnostic logging.
///
/// # Errors
/// Returns bounded protocol, native startup, or pipe failures. Closing stdin releases the
/// owned process and preserves its native history. User input is never automatically replayed.
pub async fn serve<R, W>(program: PathBuf, input: R, output: W) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    crate::local::omp::serve_acp(program, input, output).await
}
