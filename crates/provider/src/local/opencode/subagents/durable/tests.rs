use super::*;
use crate::local::opencode::PROVIDER;

fn child(id: &str, parent: &str, cwd: &str) -> NativeSubagent {
    NativeSubagent {
        persistence: None,
        id: id.into(),
        parent_id: parent.into(),
        cwd: cwd.into(),
        descriptor: json!({"id":id,"provider":PROVIDER,"status":"running",
            "updatedAt":"2026-11-11T10:00:00Z"}),
    }
}

#[test]
fn durable_terminal_outcome_settles_only_the_verified_current_child() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let path = directory.path().join("opencode.db");
    let connection = Connection::open(&path).expect("create isolated database");
    connection
        .execute_batch(
            "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT,
            time_busy INTEGER, time_idle INTEGER, idle_outcome TEXT);
            INSERT INTO session_v2 VALUES
                ('done','parent','/work',100,200,'succeeded'),
                ('failed','parent','/work',100,200,'failed'),
                ('interrupted','parent','/work',100,200,'interrupted'),
                ('unknown','parent','/work',100,200,'unknown'),
                ('resumed','parent','/work',300,200,'succeeded'),
                ('foreign','other','/work',100,200,'succeeded'),
                ('wrong-directory','parent','/other',100,200,'succeeded');",
        )
        .expect("insert native status fixtures");
    drop(connection);

    let children = [
        "done",
        "failed",
        "interrupted",
        "unknown",
        "resumed",
        "foreign",
        "wrong-directory",
    ]
    .into_iter()
    .map(|id| child(id, "parent", "/work"))
    .collect();
    let reconciled = reconcile_from_path(path, children);

    assert_eq!(reconciled[0].descriptor["status"], "completed");
    assert_eq!(reconciled[1].descriptor["status"], "failed");
    assert_eq!(reconciled[2].descriptor["status"], "canceled");
    assert!(reconciled[0].descriptor["updatedAt"] != "2026-11-11T10:00:00Z");
    for child in &reconciled[3..] {
        assert_eq!(child.descriptor["status"], "running");
    }
}

#[test]
fn missing_native_database_preserves_observed_status() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let children = vec![child("child", "parent", "/work")];

    assert_eq!(
        reconcile_from_path(directory.path().join("missing.db"), children.clone()),
        children
    );
}

#[test]
fn database_override_resolves_like_opencode() {
    let mut client = OpenCodeClient::new(PathBuf::from("opencode"));
    client
        .environment
        .insert("XDG_DATA_HOME".into(), "/data".into());

    client
        .environment
        .insert("OPENCODE_DB".into(), "custom.db".into());
    assert_eq!(
        database_path(&client),
        Some(PathBuf::from("/data/opencode/custom.db"))
    );
    client
        .environment
        .insert("OPENCODE_DB".into(), "/elsewhere/custom.db".into());
    assert_eq!(
        database_path(&client),
        Some(PathBuf::from("/elsewhere/custom.db"))
    );
    client
        .environment
        .insert("OPENCODE_DB".into(), ":memory:".into());
    assert_eq!(database_path(&client), None);
}
