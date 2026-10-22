use std::io::Write;

use super::*;
use crate::local::claude::history;

#[test]
fn native_paths_and_handles_are_scoped_to_canonical_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = ClaudeClient::new("unused".into());
    client.config_dir = Some(directory.path().join("config"));
    let cwd = directory.path().canonicalize().unwrap();
    let encoded: String = cwd
        .to_str()
        .unwrap()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    assert_eq!(
        history::project_dir(&client, cwd.to_str().unwrap()).unwrap(),
        directory.path().join("config/projects").join(encoded)
    );
    let mut handle = AgentPersistenceHandle {
        provider: "claude".into(),
        session_id: uuid::Uuid::new_v4().to_string(),
        native_handle: None,
        metadata: None,
    };
    assert!(
        history::read(&client, &handle, cwd.to_str().unwrap())
            .unwrap()
            .is_none()
    );
    handle.session_id = "../../secret".into();
    assert!(history::validate_handle(&handle).is_err());
    assert!(
        history::list(
            &client,
            &ListOptions {
                cwd: None,
                scan_limit: 0
            }
        )
        .is_err()
    );
    assert_eq!(
        history::list(
            &client,
            &ListOptions {
                cwd: None,
                scan_limit: 10
            }
        )
        .unwrap(),
        [] as [SessionDescriptor; 0]
    );
}

#[test]
fn long_native_project_names_match_sdk_hashing() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a".repeat(110)).join("b".repeat(110));
    std::fs::create_dir_all(&path).unwrap();
    let mut client = ClaudeClient::new("unused".into());
    client.config_dir = Some(root.path().join("config"));
    let native = history::project_dir(&client, path.to_str().unwrap()).unwrap();
    assert!(native.file_name().unwrap().to_str().unwrap().len() > 200);
    assert!(native.file_name().unwrap().to_str().unwrap().len() <= 208);
}

#[test]
fn history_rejects_corruption_and_cwd_mismatch_without_importing_partial_records() {
    let root = tempfile::tempdir().unwrap();
    let mut client = ClaudeClient::new("unused".into());
    client.config_dir = Some(root.path().join("config"));
    let cwd = root.path().to_str().unwrap();
    let handle = AgentPersistenceHandle {
        provider: "claude".into(),
        session_id: uuid::Uuid::new_v4().to_string(),
        native_handle: None,
        metadata: None,
    };
    let directory = history::project_dir(&client, cwd).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{}.jsonl", handle.session_id));
    let mut record = json!({"type":"user","uuid":"prompt","sessionId":handle.session_id,"cwd":cwd,
        "timestamp":"2026-09-26T00:00:00Z","message":{"content":"Hello"}});
    std::fs::write(&path, format!("{record}\n")).unwrap();
    assert!(
        history::read(&client, &handle, cwd)
            .unwrap()
            .unwrap()
            .active
    );
    for invalid in ["{broken\n".to_owned(), record.to_string(), "{}\n".into()] {
        std::fs::write(&path, invalid).unwrap();
        assert!(history::read(&client, &handle, cwd).is_err());
        assert_eq!(
            history::list(
                &client,
                &ListOptions {
                    cwd: Some(cwd.into()),
                    scan_limit: 10
                }
            )
            .unwrap(),
            [] as [SessionDescriptor; 0]
        );
    }
    record["cwd"] = json!("/not/the/workspace");
    std::fs::write(&path, format!("{record}\n")).unwrap();
    assert!(history::read(&client, &handle, cwd).is_err());
    record["cwd"] = json!(cwd);
    let mut moved = record.clone();
    moved["uuid"] = json!("moved");
    moved["cwd"] = json!(root.path().join("nested"));
    std::fs::write(&path, format!("{record}\n{moved}\n")).unwrap();
    assert!(history::read(&client, &handle, cwd).is_ok());
    record["sessionId"] = json!(uuid::Uuid::new_v4().to_string());
    std::fs::write(&path, format!("{record}\n")).unwrap();
    assert!(history::read(&client, &handle, cwd).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_history_paths_normalize_combining_marks_like_the_sdk() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cafe\u{301}");
    std::fs::create_dir(&path).unwrap();
    let mut client = ClaudeClient::new("unused".into());
    client.config_dir = Some(root.path().join("config"));
    let native = history::project_dir(&client, path.to_str().unwrap()).unwrap();
    assert!(
        native
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-caf-")
    );
}

#[test]
fn live_child_reads_ignore_only_an_unfinished_final_record() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("child.jsonl");
    std::fs::write(&path, "{\"type\":\"user\"}\n{\"partial\":").unwrap();
    assert!(history::read_records(&path).is_err());
    assert_eq!(
        history::read_active_records(&path).unwrap(),
        vec![json!({"type":"user"})]
    );
    std::fs::write(&path, "{broken}\n").unwrap();
    assert!(history::read_active_records(&path).is_err());
    std::fs::write(&path, "x".repeat(2 * 1024 * 1024)).unwrap();
    assert!(history::read_active_records(&path).is_err());
}

#[test]
fn history_input_limits_reject_large_files_and_record_counts_but_allow_blank_lines() {
    let root = tempfile::tempdir().unwrap();
    assert!(history::read_records(root.path()).is_err());
    let path = root.path().join("bounded.jsonl");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(128 * 1024 * 1024 + 1).unwrap();
    assert!(history::read_records(&path).is_err());
    std::fs::write(&path, "{}\n".repeat(65537)).unwrap();
    assert!(history::read_records(&path).is_err());
    std::fs::write(&path, "{}\n".repeat(65536)).unwrap();
    assert_eq!(history::read_records(&path).unwrap().len(), 65536);
    // Long 1M-context sessions grow past the former 32 MiB cap and must still resume.
    let record = format!("{}\n", json!({"pad": "x".repeat(2 * 1024 * 1024 - 16)}));
    std::fs::write(&path, record.repeat(17)).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 32 * 1024 * 1024);
    assert_eq!(history::read_records(&path).unwrap().len(), 17);
    std::fs::write(&path, " \t\n{}\n\n").unwrap();
    assert_eq!(history::read_records(&path).unwrap(), [json!({})]);
}

#[test]
fn completed_history_ignores_sidechains_and_preserves_the_latest_explicit_title() {
    let root = tempfile::tempdir().unwrap();
    let mut client = ClaudeClient::new("unused".into());
    client.config_dir = Some(root.path().join("config"));
    let cwd = root.path().to_str().unwrap();
    let handle = AgentPersistenceHandle {
        provider: "claude".into(),
        session_id: uuid::Uuid::new_v4().to_string(),
        native_handle: None,
        metadata: None,
    };
    let directory = history::project_dir(&client, cwd).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let records = [
        json!({"type":"user","uuid":"prompt","sessionId":handle.session_id,"cwd":cwd,"timestamp":"2026-09-26T00:00:00Z","message":{"content":"Hello"}}),
        json!({"isSidechain":true,"sessionId":"other","customTitle":"ignored"}),
        json!({"summary":"summary"}),
        json!({"customTitle":"User title"}),
        json!({"type":"result","subtype":"success","result":"Done"}),
    ];
    let mut file =
        std::fs::File::create(directory.join(format!("{}.jsonl", handle.session_id))).unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    let result = history::read(&client, &handle, cwd).unwrap().unwrap();
    assert!(!result.active);
    assert_eq!(result.descriptor.title.as_deref(), Some("User title"));
    assert!(
        result
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "Done")
    );
}
