use std::process::Command;

use rusqlite::Connection;
use serde_json::json;

#[test]
fn migration_command_copies_core_data_and_reports_the_backup() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy");
    let destination = root.path().join("desktop");
    std::fs::create_dir_all(source.join("agents")).unwrap();
    std::fs::create_dir_all(source.join("projects")).unwrap();
    std::fs::create_dir_all(destination.join("agents")).unwrap();
    std::fs::write(
        source.join("agents/agents.json"),
        serde_json::to_vec(&[json!({"id":"agent"})]).unwrap(),
    )
    .unwrap();
    std::fs::write(
        source.join("projects/projects.json"),
        serde_json::to_vec(&[json!({"id":"project"})]).unwrap(),
    )
    .unwrap();
    std::fs::write(
        source.join("projects/workspaces.json"),
        serde_json::to_vec(&[json!({"id":"workspace"})]).unwrap(),
    )
    .unwrap();
    let timeline = Connection::open(source.join("agents/timeline.sqlite3")).unwrap();
    timeline
        .execute_batch("CREATE TABLE entries(value TEXT); INSERT INTO entries VALUES ('entry');")
        .unwrap();
    drop(timeline);

    let output = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args([
            "migrate-data",
            "--source",
            source.to_str().unwrap(),
            "--data-dir",
            destination.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["summary"]["agents"], 1);
    assert_eq!(report["summary"]["projects"], 1);
    assert_eq!(report["summary"]["workspaces"], 1);
    assert_eq!(report["summary"]["timeline_rows"], 1);
    assert!(destination.join("agents/agents.json").is_file());
    assert!(destination.join("projects/workspaces.json").is_file());
    assert!(std::path::Path::new(report["backup"].as_str().unwrap()).is_dir());
}

#[test]
fn normal_start_refuses_an_incomplete_migration() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("desktop");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join(".legacy-data-migration.json"), b"{}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .args([
            "--data-dir",
            destination.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .env_remove("AIT_SERVER_TOKEN")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migration is incomplete"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
