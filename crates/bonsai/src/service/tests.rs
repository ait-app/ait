use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use secrecy::SecretString;
use serde_json::json;

use super::{DRAIN, Service, StartError, write_policy};
use crate::config::Config;
use crate::runs::WritePolicy;
use crate::store::{Store, StoreError};
use crate::testing::{MockHost, dispatch};
use crate::translate::uuid_of;
use crate::wire::{ReasonCode, RunState};

const RUNTIME_ID: &str = "rt_0123456789abcdef0123456789abcdef";
const TOKEN: &str = "fedcba9876543210fedcba9876543210";
const RUN_HEX: &str = "0123456789abcdef0123456789abcdef";
/// Bound for a prompt stop: the drain window plus generous scheduling slack, and well under
/// the 20 s handshake timeout a stop must not wait for.
const PROMPT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);

fn config(origin: &str) -> Config {
    Config::new(
        origin,
        RUNTIME_ID.to_owned(),
        SecretString::from(TOKEN.to_owned()),
    )
    .expect("valid configuration")
}

/// A loopback origin whose port was just released, so connecting to it is refused.
fn refused_origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn spawn(origin: &str, mock: &MockHost, data_dir: &Path) -> Service {
    Service::spawn(config(origin), mock.host(), data_dir).expect("service starts")
}

/// Wait up to `limit` for one connection; the accepted stream blocks with a read timeout.
fn accept_within(listener: &TcpListener, limit: Duration) -> std::io::Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + limit;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(limit))?;
                return Ok(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(POLL);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Read an HTTP request head, through the blank line that ends it.
fn read_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Accept the worker's upgrade request and leave it unanswered, as a stalled Hub would.
async fn stalled_handshake(listener: TcpListener) -> (TcpStream, String) {
    tokio::task::spawn_blocking(move || {
        let mut stream = accept_within(&listener, PROMPT).expect("the worker connects");
        let head = read_head(&mut stream).expect("request head");
        (stream, head)
    })
    .await
    .expect("accept task")
}

/// Wait for the peer to close the connection; errors if it stays open past the read timeout.
async fn closed_by_peer(mut stream: TcpStream) -> std::io::Result<usize> {
    tokio::task::spawn_blocking(move || stream.read(&mut [0_u8; 64]))
        .await
        .expect("read task")
}

#[test]
fn write_policy_marks_loopback_endpoints() {
    let cases = [
        ("http://localhost:8860", true),
        ("http://127.0.0.2:8860", true),
        ("http://[::1]:8860", true),
        ("https://localhost", true),
        ("https://bonsai.example", false),
        ("https://127.0.0.1.example", false),
    ];
    for (origin, loopback) in cases {
        assert_eq!(
            write_policy(&config(origin)),
            WritePolicy { loopback },
            "{origin}"
        );
    }
}

#[test]
fn every_store_failure_is_reported_as_storage() {
    assert_eq!(StartError::from(StoreError::Sqlite), StartError::Storage);
    assert_eq!(StartError::from(StoreError::Corrupt), StartError::Storage);
}

#[test]
fn start_errors_explain_without_detail() {
    assert_eq!(
        StartError::Storage.to_string(),
        "open the Bonsai runtime database"
    );
    assert_eq!(
        StartError::Worker.to_string(),
        "start the Bonsai runtime worker"
    );
}

#[test]
fn spawn_reports_storage_when_the_data_directory_is_a_file() {
    let file = tempfile::NamedTempFile::new().expect("a regular file");
    let mock = MockHost::new();
    let started = Service::spawn(config(&refused_origin()), mock.host(), file.path());
    assert_eq!(started.err(), Some(StartError::Storage));
    assert!(mock.executor.calls().is_empty());
}

#[test]
fn spawn_reports_storage_when_the_database_path_is_a_directory() {
    let data = tempfile::tempdir().expect("temporary data directory");
    std::fs::create_dir_all(data.path().join("bonsai/runtime.sqlite3"))
        .expect("blocking directory");
    let mock = MockHost::new();
    let started = Service::spawn(config(&refused_origin()), mock.host(), data.path());
    assert_eq!(started.err(), Some(StartError::Storage));
}

#[tokio::test(flavor = "multi_thread")]
async fn spawn_creates_the_database_and_shutdown_waits_for_the_worker() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let mock = MockHost::new();
    let service = spawn(&refused_origin(), &mock, data.path());
    assert!(data.path().join("bonsai/runtime.sqlite3").is_file());
    let stopping = Instant::now();
    tokio::time::timeout(PROMPT, service.shutdown())
        .await
        .expect("shutdown returns promptly while reconnecting");
    assert!(
        stopping.elapsed() >= DRAIN,
        "the worker drains before it exits"
    );
    assert_eq!(
        mock.executor
            .calls_of("provider.available.list.request")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_shutdown_returns_at_once() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let mock = MockHost::new();
    let service = spawn(&refused_origin(), &mock, data.path());
    tokio::time::timeout(PROMPT, service.shutdown())
        .await
        .expect("first shutdown");
    let again = Instant::now();
    service.shutdown().await;
    assert!(again.elapsed() < DRAIN, "nothing is left to wait for");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worker_dials_the_endpoint_and_shutdown_abandons_a_stalled_handshake() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let mock = MockHost::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a fake Hub");
    let port = listener.local_addr().expect("local address").port();
    let service = spawn(&format!("http://127.0.0.1:{port}"), &mock, data.path());
    let (stream, head) = stalled_handshake(listener).await;
    let stopping = Instant::now();
    tokio::time::timeout(PROMPT, service.shutdown())
        .await
        .expect("shutdown does not wait for the handshake timeout");
    let read = closed_by_peer(stream).await;
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("get /runtime http/1.1\r\n"), "{head}");
    assert!(head.contains(&format!("\r\nauthorization: bearer {TOKEN}\r\n")));
    assert!(stopping.elapsed() < PROMPT);
    assert_eq!(read.expect("the worker closed the connection"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_service_stops_the_worker() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let mock = MockHost::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a fake Hub");
    let port = listener.local_addr().expect("local address").port();
    let service = spawn(&format!("http://127.0.0.1:{port}"), &mock, data.path());
    let (stream, _) = stalled_handshake(listener).await;
    drop(service);
    let read = closed_by_peer(stream).await;
    assert_eq!(read.expect("the worker closed the connection"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn spawn_recovers_runs_left_open_in_the_data_directory() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let database = data.path().join("bonsai/runtime.sqlite3");
    let run_id = format!("r_{RUN_HEX}");
    let store = Store::open(&database).expect("existing database");
    assert!(
        store
            .insert_run(&dispatch(&run_id), "e-1", 1)
            .expect("open run")
    );
    drop(store);
    let mock = MockHost::new();
    let service = spawn(&refused_origin(), &mock, data.path());
    tokio::time::timeout(PROMPT, service.shutdown())
        .await
        .expect("shutdown");
    let run = Store::open(&database)
        .expect("reopen")
        .run(&run_id)
        .expect("read")
        .expect("run kept");
    assert_eq!(
        mock.executor.calls_of("agent.get.request"),
        vec![json!({"agentId": uuid_of(RUN_HEX)})]
    );
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::SessionLost));
}

#[tokio::test(flavor = "multi_thread")]
async fn debug_output_names_the_service_without_credentials() {
    let data = tempfile::tempdir().expect("temporary data directory");
    let mock = MockHost::new();
    let service = spawn(&refused_origin(), &mock, data.path());
    let text = format!("{service:?}");
    tokio::time::timeout(PROMPT, service.shutdown())
        .await
        .expect("shutdown");
    assert_eq!(text, "Service { .. }");
    assert!(!text.contains(TOKEN));
    assert!(!text.contains(RUNTIME_ID));
}
