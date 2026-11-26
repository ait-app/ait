//! On-demand sampling publishes completed snapshots without blocking cache readers.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::daemon::ports::host_monitor::{HostMonitorError, HostSample, HostSampler, HostSnapshot};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const DISK_INTERVAL: Duration = Duration::from_secs(30);
const HISTORY_WINDOW: Duration = Duration::from_secs(180);
const HISTORY_LIMIT: usize = 90;

/// One tracked sampler independent of the short-job budget and the published cache lock.
#[derive(Debug)]
pub(super) struct Monitor {
    worker: Mutex<HostMonitor>,
    cached: RwLock<Option<(Instant, HostSnapshot)>>,
    slots: Arc<tokio::sync::Semaphore>,
}

impl Monitor {
    pub(super) fn new(sampler: Box<dyn HostSampler>) -> Self {
        Self {
            worker: Mutex::new(HostMonitor::new(sampler)),
            cached: RwLock::new(None),
            slots: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(super) fn read(&self, include_history: bool) -> Result<HostSnapshot, HostMonitorError> {
        let cache = self.cached.read().map_err(|_| HostMonitorError)?;
        let snapshot = &cache.as_ref().ok_or(HostMonitorError)?.1;
        Ok(HostSnapshot {
            platform: snapshot.platform.clone(),
            cpu_count: snapshot.cpu_count,
            uptime_seconds: snapshot.uptime_seconds,
            current: snapshot.current.clone(),
            history: if include_history {
                snapshot.history.clone()
            } else {
                Vec::new()
            },
            disks: snapshot.disks.clone(),
        })
    }

    pub(super) fn request(self: &Arc<Self>, runtime: &model::Runtime) {
        let now = Instant::now();
        if self.cached.read().is_ok_and(|cache| {
            cache
                .as_ref()
                .is_some_and(|(at, _)| now.duration_since(*at) < SAMPLE_INTERVAL)
        }) {
            return;
        }
        let Ok(permit) = self.slots.clone().try_acquire_owned() else {
            return;
        };
        let _admission = runtime
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.cancellation.is_cancelled() {
            return;
        }
        let monitor = self.clone();
        runtime.tasks.spawn_blocking(move || {
            let _permit = permit;
            if monitor.sample(now).is_err() {
                tracing::warn!(operation = "host_sample", "Host sample unavailable");
            }
        });
    }

    fn sample(&self, now: Instant) -> Result<(), HostMonitorError> {
        let snapshot = self
            .worker
            .lock()
            .map_err(|_| HostMonitorError)?
            .read(now, true)?;
        *self.cached.write().map_err(|_| HostMonitorError)? = Some((now, snapshot));
        Ok(())
    }
}

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
