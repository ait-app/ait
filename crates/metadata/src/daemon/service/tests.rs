use std::sync::Mutex;

use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use model::storage::daemon_config::DaemonConfigStore;
use serde_json::json;

use super::*;

#[derive(Debug)]
struct Config {
    value: Mutex<Value>,
}

impl DaemonConfigStore for Config {
    fn get(&self) -> Result<Value, DaemonConfigStoreError> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn patch(&self, patch: &Value) -> Result<Value, DaemonConfigStoreError> {
        let mut value = self.value.lock().unwrap();
        value["patch"] = patch.clone();
        Ok(value.clone())
    }

    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError> {
        Ok(DaemonConfigReload {
            config: self.get()?,
            applied_paths: vec!["daemon.relay.enabled".to_owned()],
            restart_required_paths: Vec::new(),
            override_controlled_paths: Vec::new(),
        })
    }
}

fn daemon() -> Daemon {
    Daemon::new(
        DaemonRuntime {
            server_id: "srv-1".to_owned(),
            version: Some("1.2.3".to_owned()),
            pid: 42,
            executable: "/bin/server".to_owned(),
            started_at: Some("2026-09-22T00:00:00Z".to_owned()),
            listen: "127.0.0.1:6767".to_owned(),
        },
        Box::new(Config {
            value: Mutex::new(json!({"relay":{"enabled":false}})),
        }),
    )
}

#[test]
fn coordinates_config_get_patch_and_reload() {
    let daemon = daemon();
    assert_eq!(daemon.get_config().unwrap()["relay"]["enabled"], false);
    assert_eq!(
        daemon
            .set_config(&json!({"relay":{"enabled":true}}))
            .unwrap()["patch"]["relay"]["enabled"],
        true
    );
    assert_eq!(
        daemon.reload_config().unwrap().applied_paths,
        ["daemon.relay.enabled"]
    );
}

#[test]
fn status_diagnostics_and_update_are_sanitized_and_stable() {
    let daemon = daemon();
    assert_eq!(daemon.runtime().server_id, "srv-1");
    let diagnostic = daemon.diagnostics(&["daemon.get_status.request".to_owned()], "ready", &[]);
    assert!(diagnostic.contains("Server ID: srv-1"));
    assert!(diagnostic.contains("Methods: daemon.get_status.request"));
    assert!(!diagnostic.contains("token"));
    assert_eq!(
        daemon.update_result(),
        DaemonUpdate {
            success: false,
            error: Some(
                "Self-update is unavailable for this standalone Rust server installation"
                    .to_owned()
            ),
            previous_version: Some("1.2.3".to_owned()),
            new_version: None,
        }
    );
}

#[test]
fn snapshots_report_provider_availability_without_backend_secrets() {
    use model::Lifecycle;

    use crate::daemon::protocol::ProviderAvailability;
    use crate::daemon::rpc::snapshot;

    let daemon = daemon();
    let providers = vec![
        ProviderAvailability {
            provider: "codex".to_owned(),
            available: true,
            error: None,
        },
        ProviderAvailability {
            provider: "other".to_owned(),
            available: false,
            error: Some("private-token".to_owned()),
        },
    ];
    let status = snapshot(
        &daemon,
        "daemon.get_status.request",
        json!({}),
        &providers,
        (&[], Lifecycle::Ready),
    )
    .unwrap();
    assert_eq!(
        status["providers"],
        serde_json::to_value(&providers).unwrap()
    );
    let result = snapshot(
        &daemon,
        "diagnostics.request",
        json!({}),
        &providers,
        (&[], Lifecycle::Draining),
    )
    .unwrap();
    let report = result["diagnostic"].as_str().unwrap();
    for expected in [
        "Total: 2",
        "Available: 1",
        "codex: available",
        "other: unavailable",
        "Lifecycle: draining",
    ] {
        assert!(report.contains(expected), "{report}");
    }
    assert!(!report.contains("private-token"));
    assert_eq!(
        snapshot(
            &daemon,
            "unknown",
            json!({}),
            &providers,
            (&[], Lifecycle::Ready)
        ),
        Err(crate::support::error::ErrorCode::MethodNotFound)
    );
    assert_eq!(
        snapshot(
            &daemon,
            "daemon.get_status.request",
            Value::Null,
            &providers,
            (&[], Lifecycle::Ready)
        ),
        Err(crate::support::error::ErrorCode::InvalidMessage)
    );
}

#[test]
fn diagnostics_include_host_owned_evidence() {
    #[derive(Debug)]
    struct Evidence;
    impl crate::daemon::ports::DaemonDiagnostics for Evidence {
        fn report(&self) -> String {
            "bounded fixture evidence".into()
        }
    }
    let daemon = daemon().with_diagnostics(std::sync::Arc::new(Evidence));
    assert!(
        daemon
            .diagnostics(&[], "ready", &[])
            .contains("bounded fixture evidence")
    );
}

#[derive(Debug)]
struct TestHostSampler;

impl crate::daemon::ports::host_monitor::HostSampler for TestHostSampler {
    fn sample(
        &mut self,
        _: bool,
    ) -> Result<
        crate::daemon::ports::host_monitor::HostSnapshot,
        crate::daemon::ports::host_monitor::HostMonitorError,
    > {
        Ok(serde_json::from_value(json!({
            "platform":"test", "cpuCount":4, "uptimeSeconds":60,
            "current":{"at":123, "cpuPercent":null, "memoryUsedBytes":512,
                "memoryTotalBytes":1024, "rxBytesPerSecond":null, "txBytesPerSecond":null},
            "history":[], "disks":[]
        }))
        .unwrap())
    }
}

#[tokio::test]
async fn host_monitor_rpc_validates_input_and_returns_opt_in_history() {
    let daemon = daemon().with_host_sampler(Box::new(TestHostSampler));
    let runtime = monitor_runtime();
    assert!(daemon.host_stats(false).is_err());
    daemon.refresh_host_stats(&runtime);
    runtime.tasks.close();
    runtime.tasks.wait().await;
    let call = |daemon: &Daemon, params| {
        crate::daemon::rpc::validate_host_stats(&params)?;
        crate::daemon::rpc::execute(daemon, "daemon.host_stats.request", params)
            .map(|executed| executed.value)
    };
    let summary = call(&daemon, json!({})).unwrap();
    assert_eq!(summary["platform"], "test");
    assert!(summary["history"].as_array().unwrap().is_empty());
    assert!(summary["current"]["cpuPercent"].is_null());
    let detail = call(&daemon, json!({"includeHistory":true})).unwrap();
    assert_eq!(detail["history"].as_array().unwrap().len(), 1);
    for params in [
        json!([]),
        json!({"includeHistory":"yes"}),
        json!({"arbitrary":true}),
    ] {
        assert!(call(&daemon, params).is_err());
    }
}

#[test]
fn host_monitor_without_an_adapter_fails_explicitly() {
    assert!(daemon().host_stats(false).is_err());
}

/// A sampler that parks inside `sample()` until the test releases it.
///
/// Parking inside the call is what makes the lock state observable: while the sampler is here,
/// the monitor — and therefore any lock protecting it — is genuinely held.
#[derive(Debug)]
struct HeldSampler {
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl crate::daemon::ports::host_monitor::HostSampler for HeldSampler {
    fn sample(
        &mut self,
        _: bool,
    ) -> Result<
        crate::daemon::ports::host_monitor::HostSnapshot,
        crate::daemon::ports::host_monitor::HostMonitorError,
    > {
        let _ = self.entered.send(());
        let _ = self
            .release
            .recv_timeout(std::time::Duration::from_secs(10));
        TestHostSampler.sample(false)
    }
}

/// A configuration write must complete while a host sample is still in flight.
///
/// Both operations belong to one daemon, so a single lock around the whole service would make an
/// 8 ms durability write wait behind a `/proc` and `statfs` sweep that the client triggers every
/// two seconds. The sampler here blocks until released, so the write can only pass if it does not
/// queue behind the monitor's own lock.
#[tokio::test]
async fn config_patch_completes_while_a_host_sample_is_in_flight() {
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let daemon = std::sync::Arc::new(daemon().with_host_sampler(Box::new(HeldSampler {
        entered,
        release: release_rx,
    })));

    let runtime = monitor_runtime();
    daemon.refresh_host_stats(&runtime);
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the sampler should reach `sample()`");
    assert!(daemon.host_stats(false).is_err());
    assert_eq!(runtime.jobs.available_permits(), 3);
    for _ in 0..10 {
        daemon.refresh_host_stats(&runtime);
    }
    assert_eq!(runtime.tasks.len(), 1);

    // The write runs on its own thread so a blocked one fails the assertion instead of hanging.
    let (patched, patched_rx) = std::sync::mpsc::channel();
    let writing = {
        let daemon = daemon.clone();
        std::thread::spawn(move || {
            let _ = patched.send(daemon.set_config(&json!({"relay":{"enabled":true}})));
        })
    };
    let result = patched_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("a config write must not wait for an in-flight host sample");
    assert_eq!(result.unwrap()["patch"]["relay"]["enabled"], true);

    release.send(()).unwrap();
    writing.join().unwrap();
    runtime.tasks.close();
    runtime.tasks.wait().await;
    assert_eq!(daemon.host_stats(false).unwrap().current.at, 123);
}

fn monitor_runtime() -> model::Runtime {
    model::Runtime::new(model::ServerInfo {
        server_id: "host".to_owned(),
        version: None,
        instance_id: "instance".to_owned(),
        listen: "127.0.0.1:1".to_owned(),
        lifecycle: model::Lifecycle::Ready,
        protocol: model::VERSION,
        capabilities: vec![],
        implemented_capabilities: vec![],
        features: vec![],
        limits: model::Limits::default(),
    })
}
