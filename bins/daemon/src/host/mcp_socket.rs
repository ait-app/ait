//! Host-owned Unix socket listener and socket-file lifecycle for HTTP MCP.

use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::net::{UnixListener, UnixStream};

/// A bound MCP listener whose socket file is removed when its owner stops.
#[derive(Debug)]
pub(super) struct McpSocket {
    listener: UnixListener,
    file: SocketFile,
}

#[derive(Debug)]
struct SocketFile {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl McpSocket {
    /// Bind `path` with owner-only permissions, reclaiming only an inactive socket.
    /// Returns filesystem, active-listener, or bind errors without replacing other file types.
    pub(super) async fn bind(path: &Path) -> anyhow::Result<Self> {
        remove_stale(path).await?;
        let listener = UnixListener::bind(path).context("bind MCP Unix socket")?;
        let metadata = std::fs::symlink_metadata(path).context("inspect bound MCP Unix socket")?;
        let file = SocketFile {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("restrict MCP Unix socket permissions")?;
        Ok(Self { listener, file })
    }

    /// Serve the shared MCP routes until `api` drains, retaining the socket file until then.
    /// Returns HTTP serving errors; dropping this future also cleans up its owned socket file.
    pub(super) async fn serve(self, api: api::Api) -> anyhow::Result<()> {
        let Self { listener, file } = self;
        let result = axum::serve(listener, api.mcp_socket_router())
            .with_graceful_shutdown(async move { api.wait_draining().await })
            .await
            .context("serve MCP Unix socket");
        drop(file);
        result
    }
}

async fn remove_stale(path: &Path) -> anyhow::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("inspect MCP Unix socket path"),
    };
    if !metadata.file_type().is_socket() {
        bail!("MCP socket path already exists and is not a socket");
    }
    match tokio::time::timeout(Duration::from_secs(1), UnixStream::connect(path)).await {
        Ok(Err(error)) if error.kind() == ErrorKind::ConnectionRefused => {}
        Ok(Err(error)) => return Err(error).context("probe existing MCP Unix socket"),
        Ok(Ok(_)) | Err(_) => bail!("MCP socket path already has a listener"),
    }
    let current = std::fs::symlink_metadata(path).context("recheck inactive MCP Unix socket")?;
    if !current.file_type().is_socket()
        || current.dev() != metadata.dev()
        || current.ino() != metadata.ino()
    {
        bail!("MCP socket path changed while checking its listener");
    }
    std::fs::remove_file(path).context("remove inactive MCP Unix socket")
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path)
            && metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && let Err(error) = std::fs::remove_file(&self.path)
        {
            tracing::warn!(%error, "failed to remove owned MCP Unix socket");
        }
    }
}

#[cfg(test)]
mod tests;
