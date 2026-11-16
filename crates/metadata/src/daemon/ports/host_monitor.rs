//! Host sampling contract and bounded wire snapshots.

use serde::{Deserialize, Serialize};

/// One observation of the daemon machine, not the client device or an individual agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSample {
    /// UTC sampling time in milliseconds.
    pub at: i64,
    /// Aggregate CPU utilization; absent until a second recent sample is available.
    pub cpu_percent: Option<f32>,
    /// Physical memory currently used, excluding reclaimable cache where supported.
    pub memory_used_bytes: u64,
    /// Physical memory available to this operating system.
    pub memory_total_bytes: u64,
    /// Received bytes per second; absent during warmup or without supported interfaces.
    pub rx_bytes_per_second: Option<u64>,
    /// Transmitted bytes per second; absent during warmup or without supported interfaces.
    pub tx_bytes_per_second: Option<u64>,
}

/// Space available to an ordinary user on one local filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDisk {
    /// Mount point displayed to connected clients.
    pub mount: String,
    /// Total filesystem capacity.
    pub total_bytes: u64,
    /// Writable free space, excluding blocks reserved for the operating system.
    pub available_bytes: u64,
}

/// Cached resource snapshot returned through authenticated daemon RPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    /// Operating system name.
    pub platform: String,
    /// Logical processor count.
    pub cpu_count: usize,
    /// Time since host boot, when supported.
    pub uptime_seconds: Option<u64>,
    /// Latest observation.
    pub current: HostSample,
    /// Recent observations, omitted from summary requests.
    pub history: Vec<HostSample>,
    /// Local filesystem capacity, refreshed less often than CPU and memory.
    pub disks: Vec<HostDisk>,
}

/// Failure to initialize or read the operating system's resource counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("host resource sampling is unavailable")]
pub struct HostMonitorError;

/// Blocking system adapter called only from the daemon service's blocking worker.
pub trait HostSampler: Send + std::fmt::Debug {
    /// Read current counters, refreshing disks only when `refresh_disks` is true.
    ///
    /// # Errors
    /// Returns `HostMonitorError` when system counters cannot be read.
    fn sample(&mut self, refresh_disks: bool) -> Result<HostSnapshot, HostMonitorError>;
}
