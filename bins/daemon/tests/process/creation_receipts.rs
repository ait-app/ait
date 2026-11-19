use std::io::Write;
use std::time::{Duration, Instant};

use domain::creation::protocol::Kind;
use serde_json::json;

use super::transport::{connect, request};
use super::{ready, start_with_path, terminate};

#[tokio::test]
async fn oversized_creation_receipts_allow_startup_and_restore_reserved_resources() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    let path = directory.join("creations/receipts.json");
    let creations = model::creation::Creations::default();
    let admitted = creations.begin(Kind::Agent, "restored", json!({})).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let row = json!({"id":"agent:eb00bf0aba491c620ddf47bf68068be4cc52c39bf3b8b554e2c51ff74e5e915e","intent":{},"snapshot":admitted.snapshot});
    serde_json::to_writer(std::fs::File::create(&path).unwrap(), &[row]).unwrap();
    // Trailing JSON whitespace exercises the aggregate limit without large request payloads.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&vec![b' '; 33 * 1024 * 1024])
        .unwrap();

    let log = root.path().join("server.log");
    let mut process = start_with_path(&directory, &log, Some(root.path().as_os_str()));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, &["creation.subscribe.request"]).await;
    let response = request(
        &mut socket,
        "creation.subscribe.request",
        json!({"kind":"agent","idempotencyKey":"restored","subscribe":false}),
    )
    .await;
    let snapshot = &response["result"]["snapshot"];
    assert_eq!(snapshot["agentId"], json!(admitted.snapshot.agent_id));
    assert_eq!(snapshot["phase"], "failed");
    assert_eq!(snapshot["outcomeUnknown"], true);
    drop(socket);
    terminate(&mut process).await;
    assert!(std::fs::metadata(path).unwrap().len() < 64 * 1024);
}

#[tokio::test]
async fn invalid_creation_receipts_report_safe_diagnostics_and_preserve_the_file() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    let path = directory.join("creations/receipts.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let contents = r#"[{"id":"private-prompt","intent":{},"snapshot":{"kind":"secret-kind"}}]"#;
    std::fs::write(&path, contents).unwrap();
    let log = root.path().join("server.log");
    let mut process = start_with_path(&directory, &log, Some(root.path().as_os_str()));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(!status.success());
    let diagnostic = std::fs::read_to_string(log).unwrap();
    assert!(diagnostic.contains("invalid receipt JSON"), "{diagnostic}");
    assert!(
        diagnostic.contains("creations/receipts.json"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("(Data)"), "{diagnostic}");
    assert!(diagnostic.contains("line 1"), "{diagnostic}");
    assert!(diagnostic.contains("column "), "{diagnostic}");
    assert!(!diagnostic.contains("private-prompt"), "{diagnostic}");
    assert!(!diagnostic.contains("secret-kind"), "{diagnostic}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
}
