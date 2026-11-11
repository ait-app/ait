use std::sync::{Arc, Mutex};

use super::*;

#[derive(Debug)]
struct Sampler(Arc<Mutex<Vec<bool>>>);

impl HostSampler for Sampler {
    fn sample(&mut self, refresh_disks: bool) -> Result<HostSnapshot, HostMonitorError> {
        let mut calls = self.0.lock().unwrap();
        calls.push(refresh_disks);
        Ok(HostSnapshot {
            platform: "test".to_owned(),
            cpu_count: 4,
            uptime_seconds: Some(60),
            current: HostSample {
                at: i64::try_from(calls.len()).unwrap(),
                cpu_percent: Some(25.0),
                memory_used_bytes: 512,
                memory_total_bytes: 1024,
                rx_bytes_per_second: None,
                tx_bytes_per_second: None,
            },
            history: Vec::new(),
            disks: Vec::new(),
        })
    }
}

#[test]
fn shares_sampling_cache_and_only_includes_history_when_requested() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut monitor = HostMonitor::new(Box::new(Sampler(calls.clone())));
    let now = Instant::now();
    assert!(monitor.read(now, false).unwrap().history.is_empty());
    let detail = monitor
        .read(now + Duration::from_millis(100), true)
        .unwrap();
    assert_eq!(detail.history.len(), 1);
    assert_eq!(*calls.lock().unwrap(), [true]);
    monitor.read(now + SAMPLE_INTERVAL, false).unwrap();
    monitor.read(now + DISK_INTERVAL, true).unwrap();
    assert_eq!(*calls.lock().unwrap(), [true, false, true]);
}

#[test]
fn bounds_history_and_expires_observations_across_idle_periods() {
    let mut monitor = HostMonitor::new(Box::new(Sampler(Arc::new(Mutex::new(Vec::new())))));
    let now = Instant::now();
    for index in 0..100 {
        monitor.read(now + SAMPLE_INTERVAL * index, false).unwrap();
    }
    assert_eq!(
        monitor
            .read(now + Duration::from_secs(198), true)
            .unwrap()
            .history
            .len(),
        HISTORY_LIMIT
    );
    assert_eq!(
        monitor
            .read(now + Duration::from_secs(400), true)
            .unwrap()
            .history
            .len(),
        1
    );
}

#[derive(Debug)]
struct FailedSampler;
impl HostSampler for FailedSampler {
    fn sample(&mut self, _: bool) -> Result<HostSnapshot, HostMonitorError> {
        Err(HostMonitorError)
    }
}

#[test]
fn failed_sampling_does_not_publish_a_successful_timestamp() {
    let mut monitor = HostMonitor::new(Box::new(FailedSampler));
    assert_eq!(monitor.read(Instant::now(), true), Err(HostMonitorError));
    assert!(monitor.sampled_at.is_none());
    assert!(monitor.history.is_empty());
}
