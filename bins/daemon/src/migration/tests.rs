use super::*;

use rusqlite::Connection;
use serde_json::json;

fn seed(directory: &Path, agents: usize, projects: usize, workspaces: usize) {
    fs::create_dir_all(directory.join("agents")).unwrap();
    fs::create_dir_all(directory.join("projects/icons")).unwrap();
    let records = |count: usize, prefix: &str| {
        (0..count)
            .map(|index| json!({"id": format!("{prefix}-{index}")}))
            .collect::<Vec<_>>()
    };
    fs::write(
        directory.join("agents/agents.json"),
        serde_json::to_vec(&records(agents, "agent")).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("projects/projects.json"),
        serde_json::to_vec(&records(projects, "project")).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("projects/workspaces.json"),
        serde_json::to_vec(&records(workspaces, "workspace")).unwrap(),
    )
    .unwrap();
    fs::write(directory.join("projects/icons/icon.png"), b"icon").unwrap();
    let timeline = Connection::open(directory.join("agents/timeline.sqlite3")).unwrap();
    timeline
        .execute_batch("CREATE TABLE entries(value TEXT);")
        .unwrap();
    if agents > 0 {
        timeline
            .execute("INSERT INTO entries VALUES ('entry')", [])
            .unwrap();
    }
}

#[test]
fn migration_preserves_destination_identity_and_keeps_recoverable_backup() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy");
    let destination = root.path().join("desktop");
    seed(&source, 3, 2, 4);
    seed(&destination, 0, 0, 0);
    let destination_id = InstanceLease::acquire(&destination).unwrap().server_id;

    let report = migrate_data(&source, &destination).unwrap();

    assert_eq!(report.summary.agents, 3);
    assert_eq!(report.summary.projects, 2);
    assert_eq!(report.summary.workspaces, 4);
    assert_eq!(report.summary.timeline_rows, 1);
    assert_eq!(summarize(&destination).unwrap(), report.summary);
    assert_eq!(summarize(&source).unwrap(), report.summary);
    assert!(report.backup.join("agents/timeline.sqlite3").is_file());
    assert!(report.backup.join("projects/projects.json").is_file());
    assert_eq!(
        InstanceLease::acquire(&destination).unwrap().server_id,
        destination_id
    );
    assert_eq!(
        fs::read(destination.join("projects/icons/icon.png")).unwrap(),
        b"icon"
    );
}

#[test]
fn migration_rejects_nonempty_destination_without_changing_it() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy");
    let destination = root.path().join("desktop");
    seed(&source, 3, 2, 4);
    seed(&destination, 1, 0, 0);
    let before = fs::read(destination.join("agents/agents.json")).unwrap();

    let error = migrate_data(&source, &destination).unwrap_err();

    assert!(error.to_string().contains("already contains"));
    assert_eq!(
        fs::read(destination.join("agents/agents.json")).unwrap(),
        before
    );
}

#[test]
fn migration_rejects_a_running_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy");
    let destination = root.path().join("desktop");
    seed(&source, 1, 1, 1);
    seed(&destination, 0, 0, 0);
    let _source_lease = InstanceLease::acquire(&source).unwrap();

    let error = migrate_data(&source, &destination).unwrap_err();

    assert!(format!("{error:#}").contains("lock legacy data directory"));
    assert_eq!(summarize(&destination).unwrap().agents, 0);
}
