//! Private service discovery. Records confer connection access, never process ownership.

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};

use crate::instance::InstanceLease;

/// Atomically publish this service's authenticated connection record in `directory`.
/// Returns an I/O error if private creation, serialization or publication fails.
pub(super) fn publish(
    directory: &Path,
    address: SocketAddr,
    instance: &InstanceLease,
    token: &SecretString,
) -> anyhow::Result<()> {
    let destination = directory.join("service-connection.json");
    let temporary = directory.join(format!(".service-connection.{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .context("create service connection record")?;
        let record = serde_json::json!({
            "version": 1,
            "managementMode": "external",
            "serverId": instance.server_id,
            "instanceId": instance.instance_id,
            "pid": std::process::id(),
            "startedAt": chrono::Utc::now().to_rfc3339(),
            "listen": address.to_string(),
            "token": token.expose_secret(),
        });
        serde_json::to_writer(&mut file, &record).context("write service connection record")?;
        file.write_all(b"\n")
            .context("finish service connection record")?;
        file.sync_all().context("sync service connection record")?;
        std::fs::rename(&temporary, &destination).context("publish service connection record")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    // Keep the record after exit: service unavailability must never authorize client spawn.
    result
}

#[cfg(test)]
mod tests;
