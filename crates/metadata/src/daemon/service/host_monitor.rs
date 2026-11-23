//! Shared on-demand cache; no timer or sampling task survives its readers.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::daemon::ports::host_monitor::{HostMonitorError, HostSample, HostSampler, HostSnapshot};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const DISK_INTERVAL: Duration = Duration::from_secs(30);
const HISTORY_WINDOW: Duration = Duration::from_secs(180);
const HISTORY_LIMIT: usize = 90;

#[derive(Debug)]
pub(super) struct HostMonitor {
    sampler: Box<dyn HostSampler>,
    cached: Option<HostSnapshot>,
    sampled_at: Option<Instant>,
    disks_at: Option<Instant>,
    history: VecDeque<(Instant, HostSample)>,
}

impl HostMonitor {
    pub(super) fn new(sampler: Box<dyn HostSampler>) -> Self {
        Self {
            sampler,
            cached: None,
            sampled_at: None,
            disks_at: None,
            history: VecDeque::with_capacity(HISTORY_LIMIT),
        }
    }

    pub(super) fn read(
        &mut self,
        now: Instant,
        include_history: bool,
    ) -> Result<HostSnapshot, HostMonitorError> {
        if self
            .sampled_at
            .is_none_or(|last| now.duration_since(last) >= SAMPLE_INTERVAL)
        {
            let refresh_disks = self
                .disks_at
                .is_none_or(|last| now.duration_since(last) >= DISK_INTERVAL);
            let mut snapshot = self.sampler.sample(refresh_disks)?;
            snapshot.history.clear();
            self.history
                .retain(|(at, _)| now.duration_since(*at) < HISTORY_WINDOW);
            if self.history.len() == HISTORY_LIMIT {
                self.history.pop_front();
            }
            self.history.push_back((now, snapshot.current.clone()));
            self.cached = Some(snapshot);
            self.sampled_at = Some(now);
            if refresh_disks {
                self.disks_at = Some(now);
            }
        }
        let mut snapshot = self.cached.clone().ok_or(HostMonitorError)?;
        if include_history {
            snapshot.history = self
                .history
                .iter()
                .map(|(_, sample)| sample.clone())
                .collect();
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests;
