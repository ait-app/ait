use serde_json::json;

use super::*;

const HEADER: &str =
    r#"{"type":"session","version":3,"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}"#;

fn entry(id: &str, parent: Option<&str>) -> String {
    json!({"type":"message","id":id,"parentId":parent,"timestamp":"2026-10-21T00:00:01Z",
        "message":{"role":"user","timestamp":1000,"content":id}})
    .to_string()
}

fn ids(lines: &[String]) -> Vec<String> {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();
    read(&path)
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_last_entry_that_names_no_child_is_the_leaf_without_an_active_leaf() {
    let lines = [
        HEADER.to_owned(),
        entry("a", None),
        entry("b", Some("a")),
        entry("abandoned", Some("a")),
        entry("c", Some("b")),
    ];

    assert_eq!(ids(&lines), ["a", "b", "c"]);
}

#[test]
fn an_active_leaf_on_the_last_entry_selects_the_branch_omp_returned_to() {
    let usage = |leaf: Value| {
        json!({"type":"model_usage","id":"usage","parentId":"b","activeLeafId":leaf,
            "timestamp":"2026-10-21T00:00:02Z"})
        .to_string()
    };
    let base = [
        HEADER.to_owned(),
        entry("a", None),
        entry("b", Some("a")),
        entry("other", Some("a")),
    ];
    let with = |last: String| {
        let mut lines = base.to_vec();
        lines.push(last);
        lines
    };

    // OMP 18.18.8's `appendModelUsage` records the leaf it kept on an off-branch entry.
    assert_eq!(ids(&with(usage(json!("other")))), ["a", "other"]);
    assert!(ids(&with(usage(Value::Null))).is_empty());
    // A dangling active leaf falls back to Paseo's rule.
    assert_eq!(ids(&with(usage(json!("missing")))), ["a", "b", "usage"]);
}

#[test]
fn malformed_partial_duplicate_dangling_and_cyclic_entries_are_tolerated() {
    let lines = [
        r#"{"type":"title","v":1,"title":"Named","pad":"  "}"#.to_owned(),
        HEADER.to_owned(),
        entry("a", None),
        "{not json".to_owned(),
        entry("b", Some("a")),
        // A later duplicate replaces the earlier entry.
        entry("b", Some("a")),
        entry("c", Some("b")),
        r#"{"type":"message","id":"partial","parentId":"c","mess"#.to_owned(),
    ];
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    std::fs::write(&path, lines.join("\r\n")).unwrap();

    let snapshot = read(&path).unwrap();

    assert_eq!(snapshot.title.as_deref(), Some("Named"));
    assert_eq!(snapshot.header["id"], "id");
    assert_eq!(snapshot.entries.len(), 3);
    assert_eq!(
        ids(&[HEADER.to_owned(), entry("x", Some("missing"))]),
        ["x"]
    );
    assert_eq!(
        ids(&[
            HEADER.to_owned(),
            entry("x", Some("y")),
            entry("y", Some("x"))
        ]),
        ["x", "y"]
    );
}

#[test]
fn a_file_without_a_usable_header_or_regular_contents_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    for header in [
        r#"{"type":"session","version":3,"cwd":"/tmp"}"#,
        r#"{"type":"session","version":3,"id":"id"}"#,
        r#"{"type":"session","version":3,"id":"","cwd":"/tmp"}"#,
    ] {
        std::fs::write(&path, format!("{header}\n{}\n", entry("a", None))).unwrap();
        assert!(matches!(read(&path), Err(AgentSessionError::Failed)));
    }
    // OMP migrates older formats itself, so the version is not checked.
    std::fs::write(
        &path,
        r#"{"type":"session","version":2,"id":"id","cwd":"/tmp"}"#,
    )
    .unwrap();
    assert!(read(&path).unwrap().entries.is_empty());
    assert!(matches!(read(root.path()), Err(AgentSessionError::Failed)));
    assert!(matches!(
        read(&root.path().join("missing.jsonl")),
        Err(AgentSessionError::Unavailable)
    ));
}
