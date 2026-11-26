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

#[derive(Debug)]
struct BlockingAfterFirst {
    calls: usize,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl HostSampler for BlockingAfterFirst {
    fn sample(&mut self, _: bool) -> Result<HostSnapshot, HostMonitorError> {
        self.calls += 1;
        if self.calls > 1 {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
        }
        let mut snapshot = Sampler(Arc::new(Mutex::new(Vec::new()))).sample(false)?;
        snapshot.current.at = i64::try_from(self.calls).unwrap();
        Ok(snapshot)
    }
}

#[test]
fn reads_keep_the_previous_sample_and_timestamp_while_os_sampling_is_blocked() {
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let monitor = Arc::new(Monitor::new(Box::new(BlockingAfterFirst {
        calls: 0,
        entered,
        release: release_rx,
    })));
    let now = Instant::now();
    monitor.sample(now).unwrap();
    let sampling = {
        let monitor = monitor.clone();
        std::thread::spawn(move || monitor.sample(now + SAMPLE_INTERVAL))
    };
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let cached = monitor.read(true).unwrap();
    assert_eq!(cached.current.at, 1);
    assert_eq!(cached.history.len(), 1);
    assert!(monitor.read(false).unwrap().history.is_empty());
    release.send(()).unwrap();
    sampling.join().unwrap().unwrap();
    assert_eq!(monitor.read(true).unwrap().current.at, 2);
    assert_eq!(monitor.read(true).unwrap().history.len(), 2);
}
