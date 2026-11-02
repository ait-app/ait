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
    fs::create_dir(source.join("worktrees")).unwrap();
    let destination_lease = InstanceLease::acquire(&destination).unwrap();
    let destination_id = destination_lease.server_id;
    drop(destination_lease);

    let report = migrate_data(&source, &destination).unwrap();

    assert_eq!(report.summary.agents, 3);
    assert_eq!(report.summary.projects, 2);
    assert_eq!(report.summary.workspaces, 4);
    assert_eq!(report.summary.timeline_rows, 1);
    assert_eq!(summarize(&destination).unwrap(), report.summary);
    assert_eq!(summarize(&source).unwrap(), report.summary);
    assert!(report.backup.join("agents/timeline.sqlite3").is_file());
    assert!(report.backup.join("projects/projects.json").is_file());
    let migrated_lease = InstanceLease::acquire(&destination).unwrap();
    assert_eq!(migrated_lease.server_id, destination_id);
    drop(migrated_lease);
    assert_eq!(
        fs::read(destination.join("projects/icons/icon.png")).unwrap(),
        b"icon"
    );
    let roots = read_legacy_worktree_roots(
        &destination
            .join("projects")
            .join(LEGACY_WORKTREE_ROOTS_FILENAME),
    )
    .unwrap();
    assert_eq!(roots, [source.join("worktrees").canonicalize().unwrap()]);
    assert!(!destination.join(JOURNAL_FILENAME).exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&report.backup).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(
                destination
                    .join("projects")
                    .join(LEGACY_WORKTREE_ROOTS_FILENAME)
            )
            .unwrap()
            .permissions()
            .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn migration_recovers_an_interrupted_publication_before_retrying() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("legacy");
    let destination = root.path().join("desktop");
    seed(&source, 3, 2, 4);
    seed(&destination, 0, 0, 0);
    let original_agents = fs::read(destination.join("agents/agents.json")).unwrap();
    let source = source.canonicalize().unwrap();
    let destination = destination.canonicalize().unwrap();
    let staging = root.path().join(".ait-data-migration-interrupted");
    let backup = root
        .path()
        .join(".ait-server-desktop-before-migration-interrupted");
    create_private_directory(&staging, false).unwrap();
    create_private_directory(&backup, false).unwrap();
    stage_data(&source, &staging).unwrap();
    fs::rename(destination.join("agents"), backup.join("agents")).unwrap();
    fs::rename(staging.join("agents"), destination.join("agents")).unwrap();
    write_private_json(
        &destination.join(JOURNAL_FILENAME),
        &MigrationJournal {
            source: source.clone(),
            staging: staging.clone(),
            backup: backup.clone(),
        },
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&staging).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(destination.join(JOURNAL_FILENAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    assert!(ensure_complete(&destination).is_err());
    let report = migrate_data(&source, &destination).unwrap();

    assert_eq!(report.summary, summarize(&source).unwrap());
    assert_eq!(summarize(&destination).unwrap(), report.summary);
    assert_eq!(
        fs::read(report.backup.join("agents/agents.json")).unwrap(),
        original_agents
    );
    assert!(!destination.join(JOURNAL_FILENAME).exists());
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
