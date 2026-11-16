//! Operating-system resource adapter. Does not enumerate processes or read their arguments.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sysinfo::{DiskRefreshKind, Disks, Networks, System};

use crate::daemon::ports::host_monitor::{
    HostDisk, HostMonitorError, HostSample, HostSampler, HostSnapshot,
};

/// Lazily initialized CPU, memory, network and local-disk sampler.
#[derive(Debug, Default)]
pub struct LocalHostSampler {
    counters: Option<Counters>,
}

#[derive(Debug)]
struct Counters {
    system: System,
    networks: Option<Networks>,
    disks: Option<Disks>,
    previous: Option<Instant>,
    disk_snapshot: Vec<HostDisk>,
}

impl HostSampler for LocalHostSampler {
    fn sample(&mut self, refresh_disks: bool) -> Result<HostSnapshot, HostMonitorError> {
        if self.counters.is_none() {
            self.counters = Some(Counters {
                system: System::new().map_err(|_| HostMonitorError)?,
                networks: Networks::new().ok(),
                disks: Disks::new().ok(),
                previous: None,
                disk_snapshot: Vec::new(),
            });
        }
        self.counters
            .as_mut()
            .ok_or(HostMonitorError)?
            .sample(refresh_disks)
    }
}

impl Counters {
    fn sample(&mut self, refresh_disks: bool) -> Result<HostSnapshot, HostMonitorError> {
        let now = Instant::now();
        let elapsed = self.previous.map(|previous| now.duration_since(previous));
        // A returning viewer must not see a long idle interval presented as a live CPU/net rate.
        let recent = elapsed.filter(|elapsed| {
            *elapsed >= Duration::from_millis(200) && *elapsed <= Duration::from_secs(10)
        });
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        if self.system.total_memory() == 0 {
            return Err(HostMonitorError);
        }
        let (rx, tx) = self.network_rates(recent);
        if refresh_disks {
            self.refresh_disks();
        }
        self.previous = Some(now);
        Ok(HostSnapshot {
            platform: std::env::consts::OS.to_owned(),
            cpu_count: self.system.cpus().len(),
            uptime_seconds: System::uptime().ok(),
            current: HostSample {
                at: chrono::Utc::now().timestamp_millis(),
                cpu_percent: recent.map(|_| self.system.global_cpu_usage().clamp(0.0, 100.0)),
                memory_used_bytes: self.system.used_memory(),
                memory_total_bytes: self.system.total_memory(),
                rx_bytes_per_second: rx,
                tx_bytes_per_second: tx,
            },
            history: Vec::new(),
            disks: self.disk_snapshot.clone(),
        })
    }

    fn network_rates(&mut self, elapsed: Option<Duration>) -> (Option<u64>, Option<u64>) {
        let Some(networks) = &mut self.networks else {
            return (None, None);
        };
        networks.refresh(true);
        let mut totals = None;
        for (name, network) in networks.iter() {
            if !include_interface(&name.to_string_lossy()) {
                continue;
            }
            let (rx, tx) = totals.get_or_insert((0_u64, 0_u64));
            *rx = rx.saturating_add(network.received());
            *tx = tx.saturating_add(network.transmitted());
        }
        match (totals, elapsed) {
            (Some((rx, tx)), Some(elapsed)) => (
                Some(bytes_per_second(rx, elapsed)),
                Some(bytes_per_second(tx, elapsed)),
            ),
            _ => (None, None),
        }
    }

    fn refresh_disks(&mut self) {
        let Some(disks) = &mut self.disks else {
            return;
        };
        disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
        let mut devices = HashSet::new();
        self.disk_snapshot = disks
            .iter()
            .filter(|disk| {
                // Show the root APFS container once; other platforms deduplicate device mounts.
                let local = if cfg!(target_os = "macos") {
                    disk.mount_point() == std::path::Path::new("/")
                } else {
                    !matches!(
                        disk.file_system().to_str(),
                        Some("nfs" | "nfs4" | "cifs" | "smbfs" | "squashfs" | "iso9660")
                    )
                };
                local && disk.total_space() > 0 && devices.insert(disk.name().to_owned())
            })
            .map(|disk| HostDisk {
                mount: disk.mount_point().to_string_lossy().into_owned(),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
            })
            .collect();
        self.disk_snapshot
            .sort_by(|left, right| left.mount.cmp(&right.mount));
    }
}

fn include_interface(name: &str) -> bool {
    ![
        "lo", "veth", "docker", "br-", "virbr", "bridge", "utun", "awdl", "llw",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn bytes_per_second(bytes: u64, elapsed: Duration) -> u64 {
    u64::try_from(u128::from(bytes) * 1000 / elapsed.as_millis().max(1)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
