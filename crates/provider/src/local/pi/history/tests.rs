use super::*;

#[cfg(unix)]
mod resume;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec};

const SESSION: &str = "019a0000-0000-7000-8000-000000000001";

fn records(path: &Path) -> Result<Vec<Value>, AgentSessionError> {
    HistoryReader::open(path)?.records()
}

fn fixture() -> (tempfile::TempDir, PiClient, PathBuf, String) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let mut client = PiClient::new(directory.path().join("missing-pi"));
    client.session_environment = SessionEnvironment::isolated(directory.path(), directory.path());
    let folder = project_directory(&directory.path().join("sessions"), Path::new(&cwd)).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(format!("2026-10-18_{SESSION}.jsonl"));
    (directory, client, file, cwd)
}

fn write(file: &Path, cwd: &str, entries: Vec<Value>) {
    let header = json!({"type":"session","version":3,"id":SESSION,"cwd":cwd,
        "timestamp":"2026-10-18T00:00:00Z"});
    let mut lines = vec![header.to_string()];
    lines.extend(entries.into_iter().map(|mut entry| {
        entry["timestamp"] = json!("2026-10-18T01:00:00Z");
        entry.to_string()
    }));
    std::fs::write(file, format!("{}\n", lines.join("\n"))).unwrap();
}

fn write_history_with_record_count(file: &Path, cwd: &str, record_count: usize) {
    let header = json!({"type":"session","version":3,"id":SESSION,"cwd":cwd,
        "timestamp":"2026-10-18T00:00:00Z"});
    let header = header.to_string();
    let record = "{\"timestamp\":\"2026-10-18T01:00:00Z\"}\n";
    let mut history = String::with_capacity(header.len() + 1 + record.len() * record_count);
    history.push_str(&header);
    history.push('\n');
    history.extend(std::iter::repeat_n(record, record_count));
    std::fs::write(file, history).unwrap();
}

fn user(id: &str, parent: Value, text: &str) -> Value {
    let mut entry = json!({"type":"message","id":id,"message":{"role":"user","content":text}});
    entry["parentId"] = parent;
    entry
}

fn assistant(id: &str, parent: &str, blocks: Value) -> Value {
    let mut entry = json!({"type":"message","id":id,"parentId":parent,"message":{
        "role":"assistant","provider":"fixture","model":"model","stopReason":"stop"}});
    entry["message"]["content"] = blocks;
    entry
}

fn handle() -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: PROVIDER.into(),
        session_id: SESSION.into(),
        native_handle: None,
        metadata: None,
    }
}

#[test]
fn listing_preserves_distinct_projects_with_colliding_session_directories() {
    let (directory, client, _, _) = fixture();
    let root = directory.path().canonicalize().unwrap();
    let projects = [root.join("foo-bar"), root.join("foo/bar")];
    let sessions_root = directory.path().join("sessions");
    let session_directory = project_directory(&sessions_root, &projects[0]).unwrap();
    assert_eq!(
        session_directory,
        project_directory(&sessions_root, &projects[1]).unwrap()
    );
    std::fs::create_dir_all(&session_directory).unwrap();
    let session_ids = [SESSION, "019a0000-0000-7000-8000-000000000002"];
    for (project, session_id) in projects.iter().zip(session_ids) {
        std::fs::create_dir_all(project).unwrap();
        let file = session_directory.join(format!("2026-10-18_{session_id}.jsonl"));
        write(
            &file,
            project.to_str().unwrap(),
            vec![user("u1", Value::Null, "hello")],
        );
        let history = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, history.replace(SESSION, session_id)).unwrap();
    }

    let sessions = list(
        &client,
        &ListOptions {
            cwd: None,
            scan_limit: 10,
        },
    )
    .unwrap();

    assert_eq!(sessions.len(), 2);
    for (project, session_id) in projects.iter().zip(session_ids) {
        let session = sessions
            .iter()
            .find(|session| session.provider_handle_id == session_id)
            .unwrap();
        assert_eq!(Path::new(&session.cwd), project);
        let mut handle = handle();
        handle.session_id = session_id.into();
        assert!(inspect(&client, &handle, &session.cwd).is_ok());
    }
}

#[test]
fn expands_bare_tilde_to_the_home_directory() {
    let home = Path::new("/home/pi-user");

    assert_eq!(
        agent_directory(Some(OsStr::new("~")), Some(home.as_os_str())).unwrap(),
        home
    );
    assert_eq!(
        agent_directory(Some(OsStr::new("~")), None),
        Err(AgentSessionError::Unavailable)
    );
}

#[cfg(unix)]
#[test]
fn backslash_prefixed_tilde_remains_literal_on_unix() {
    let home = Path::new("/home/pi-user");

    assert_eq!(
        agent_directory(Some(OsStr::new(r"~\sessions")), Some(home.as_os_str())).unwrap(),
        PathBuf::from(r"~\sessions")
    );
}

#[test]
fn native_session_directory_precedence_matches_pi() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let agent = directory.path().join("agent");
    std::fs::create_dir_all(project.join(".pi")).unwrap();
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("settings.json"),
        r#"{"sessionDir":"global-sessions"}"#,
    )
    .unwrap();
    let mut client = PiClient::new(directory.path().join("missing-pi"));
    client.session_environment = SessionEnvironment::isolated(directory.path(), &agent);

    assert_eq!(
        session_directory(&client, &project).unwrap(),
        project.join("global-sessions")
    );
    std::fs::write(
        project.join(".pi/settings.json"),
        r#"{"sessionDir":"project-sessions"}"#,
    )
    .unwrap();
    assert_eq!(
        session_directory(&client, &project).unwrap(),
        project.join("project-sessions")
    );
    client.session_environment.session_directory = Some("~/environment-sessions".into());
    assert_eq!(
        session_directory(&client, &project).unwrap(),
        directory.path().join("environment-sessions")
    );
}

#[test]
fn file_url_session_paths_are_normalized_like_pi() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let agent = directory.path().join("agent root");
    let environment_sessions = directory.path().join("environment sessions");
    let settings_sessions = directory.path().join("settings sessions");
    std::fs::create_dir_all(project.join(".pi")).unwrap();
    std::fs::create_dir_all(&agent).unwrap();
    let mut client = PiClient::new(directory.path().join("missing-pi"));
    client.session_environment = SessionEnvironment::isolated(directory.path(), &agent);

    client.session_environment.agent_directory =
        Some(Url::from_file_path(&agent).unwrap().as_str().into());
    assert_eq!(
        session_directory(&client, &project).unwrap(),
        project_directory(&agent.join("sessions"), &project).unwrap()
    );

    client.session_environment.session_directory = Some(
        Url::from_file_path(&environment_sessions)
            .unwrap()
            .as_str()
            .into(),
    );
    assert_eq!(
        session_directory(&client, &project).unwrap(),
        environment_sessions
    );

    client.session_environment.session_directory = None;
    let settings_url = Url::from_file_path(&settings_sessions).unwrap();
    std::fs::write(
        project.join(".pi/settings.json"),
        json!({"sessionDir": settings_url.as_str()}).to_string(),
    )
    .unwrap();
    assert_eq!(
        session_directory(&client, &project).unwrap(),
        settings_sessions
    );
}

#[test]
fn host_home_override_selects_the_default_pi_store() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("host-home");
    let fallback = directory.path().join("profile-home");
    let project = directory.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let variables = BTreeMap::from([
        ("HOME", home.as_os_str().to_owned()),
        ("USERPROFILE", fallback.as_os_str().to_owned()),
    ]);
    let mut client = PiClient::new(directory.path().join("missing-pi"));
    client.session_environment =
        SessionEnvironment::capture(|name| variables.get(name).cloned(), directory.path().into());

    assert_eq!(
        session_directory(&client, &project).unwrap(),
        project_directory(&home.join(".pi/agent/sessions"), &project).unwrap()
    );
}

#[test]
fn default_project_directory_lexically_normalizes_the_cwd() {
    let root = Path::new("/host/sessions");

    assert_eq!(
        project_directory(root, Path::new("/repo/./parent/../project/")).unwrap(),
        project_directory(root, Path::new("/repo/project")).unwrap()
    );
}

#[test]
fn listing_reads_a_flat_custom_session_directory() {
    let (directory, mut client, _file, cwd) = fixture();
    let custom = directory.path().join("custom-sessions");
    std::fs::create_dir(&custom).unwrap();
    let file = custom.join(format!("2026-10-18_{SESSION}.jsonl"));
    write(&file, &cwd, vec![user("u", Value::Null, "custom")]);
    client.session_environment.session_directory = Some(custom.into_os_string());

    let sessions = list(
        &client,
        &ListOptions {
            cwd: Some(cwd),
            scan_limit: 10,
        },
    )
    .unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].first_prompt_preview.as_deref(), Some("custom"));
}

#[test]
fn per_agent_environment_does_not_move_the_host_session_store() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let agent = directory.path().join("agent");
    let mut client = PiClient::new(directory.path().join("missing-pi"));
    client.session_environment = SessionEnvironment::isolated(directory.path(), &agent);
    client.environment = crate::ports::environment::AgentEnvironment::try_from(BTreeMap::from([
        ("HOME".into(), "/ephemeral/home".into()),
        (
            "PI_CODING_AGENT_SESSION_DIR".into(),
            "/ephemeral/sessions".into(),
        ),
    ]))
    .unwrap();

    assert_eq!(
        session_directory(&client, &project).unwrap(),
        project_directory(&agent.join("sessions"), &project).unwrap()
    );
}

#[tokio::test]
async fn lists_and_imports_only_current_branch_without_launching_pi() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            json!({"type":"message","id":"system","parentId":null,"message":{"role":"system","content":"native instructions"}}),
            user("u1", json!("system"), "first"),
            assistant("a1", "u1", json!([{"type":"text","text":"old branch"}])),
            user("u2", json!("a1"), "discarded"),
            assistant(
                "a2",
                "u1",
                json!([{"type":"thinking","thinking":"reason"},{"type":"text","text":"current"}]),
            ),
            json!({"type":"session_info","id":"name","parentId":"a2","name":"Imported title"}),
        ],
    );
    let before = std::fs::read(&file).unwrap();
    let sessions = client
        .list_sessions(&ListOptions {
            cwd: Some(cwd.clone()),
            scan_limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title.as_deref(), Some("Imported title"));
    assert_eq!(sessions[0].last_prompt_preview.as_deref(), Some("first"));
    let history = client.inspect_session(&handle(), &cwd).await.unwrap();
    assert!(!history.active);
    assert_eq!(history.entries.len(), 3);
    assert_eq!(history.entries[2].item["text"], "current");
    assert_eq!(history.entries[0].timestamp, "2026-10-18T01:00:00Z");
    assert_eq!(history.config.model.as_deref(), Some("fixture/model"));
    assert_eq!(history.resume_metadata["sessionFile"], json!(file));
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(!client.program.exists());
}

#[test]
fn title_uses_the_latest_session_info_on_the_current_branch() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "first"),
            json!({"type":"session_info","id":"original","parentId":"u","name":"Original"}),
            assistant(
                "abandoned",
                "original",
                json!([{"type":"text","text":"old branch"}]),
            ),
            json!({"type":"session_info","id":"renamed","parentId":"abandoned","name":"Experiment"}),
            assistant(
                "current",
                "original",
                json!([{"type":"text","text":"current branch"}]),
            ),
        ],
    );

    let sessions = list(
        &client,
        &ListOptions {
            cwd: Some(cwd.clone()),
            scan_limit: 10,
        },
    )
    .unwrap();
    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert_eq!(sessions[0].title.as_deref(), Some("Original"));
    assert_eq!(history.descriptor.title.as_deref(), Some("Original"));
    assert_eq!(
        history.entries.last().unwrap().item["text"],
        "current branch"
    );
}

#[test]
fn inspection_resolves_a_renamed_session_from_its_header_identity() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "hello")]);
    let imported = inspect(&client, &handle(), &cwd).unwrap();
    let mut imported_handle = handle();
    imported_handle.metadata = Some(imported.resume_metadata);
    let renamed = file.with_file_name("renamed-session.jsonl");
    std::fs::rename(file, &renamed).unwrap();

    let history = inspect(&client, &imported_handle, &cwd).unwrap();

    assert_eq!(history.descriptor.provider_handle_id, SESSION);
    assert_eq!(history.resume_metadata["sessionFile"], json!(renamed));
}

#[test]
fn inspection_ignores_a_saved_file_outside_the_session_store() {
    let (directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "stored")]);
    let outside = directory.path().join("outside.jsonl");
    write(&outside, &cwd, vec![user("u", Value::Null, "outside")]);
    let mut imported_handle = handle();
    imported_handle.metadata = Some(BTreeMap::from([("sessionFile".into(), json!(outside))]));

    let history = inspect(&client, &imported_handle, &cwd).unwrap();

    assert_eq!(history.entries[0].item["text"], "stored");
    assert_eq!(history.resume_metadata["sessionFile"], json!(file));
}

#[test]
fn inspection_rejects_duplicate_header_identities_regardless_of_file_order() {
    use std::time::{Duration, UNIX_EPOCH};

    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "original")]);
    let duplicate = file.with_file_name(format!("copy_{SESSION}.jsonl"));
    write(&duplicate, &cwd, vec![user("copy", Value::Null, "copied")]);

    for (original_modified, duplicate_modified) in [(1, 2), (2, 1)] {
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(original_modified))
            .unwrap();
        File::options()
            .write(true)
            .open(&duplicate)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(duplicate_modified))
            .unwrap();
        assert_eq!(
            inspect(&client, &handle(), &cwd).unwrap_err(),
            AgentSessionError::Rejected
        );
    }
}

#[test]
fn inspection_does_not_read_sessions_from_other_projects() {
    let (directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "original")]);
    let other_cwd = directory.path().join("other-project");
    std::fs::create_dir(&other_cwd).unwrap();
    let other_directory =
        project_directory(&directory.path().join("sessions"), &other_cwd).unwrap();
    std::fs::create_dir_all(&other_directory).unwrap();
    let duplicate = other_directory.join(format!("copy_{SESSION}.jsonl"));
    write(
        &duplicate,
        other_cwd.to_str().unwrap(),
        vec![user("copy", Value::Null, "copied")],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert_eq!(history.entries[0].item["text"], "original");
}

#[test]
fn inspection_reuses_the_saved_session_file() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "original")]);
    let imported = inspect(&client, &handle(), &cwd).unwrap();
    let duplicate = file.with_file_name(format!("copy_{SESSION}.jsonl"));
    write(&duplicate, &cwd, vec![user("copy", Value::Null, "copied")]);
    let mut imported_handle = handle();
    imported_handle.metadata = Some(imported.resume_metadata);

    let refreshed = inspect(&client, &imported_handle, &cwd).unwrap();

    assert_eq!(refreshed.entries[0].item["text"], "original");
    assert_eq!(refreshed.resume_metadata["sessionFile"], json!(file));
}

#[cfg(unix)]
#[test]
fn missing_saved_file_fallback_does_not_scan_other_project_stores() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "project session")]);
    let inaccessible = directory.path().join("sessions/--unrelated--");
    std::fs::create_dir(&inaccessible).unwrap();
    std::fs::set_permissions(&inaccessible, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut imported_handle = handle();
    imported_handle.metadata = Some(BTreeMap::from([(
        "sessionFile".into(),
        json!(file.with_file_name("missing.jsonl")),
    )]));

    let history = inspect(&client, &imported_handle, &cwd);
    std::fs::set_permissions(&inaccessible, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(history.unwrap().entries[0].item["text"], "project session");
}

#[test]
fn listing_rejects_out_of_range_scan_limits_before_directory_filtering() {
    let (directory, client, _file, cwd) = fixture();
    let missing = directory
        .path()
        .join("missing")
        .to_str()
        .unwrap()
        .to_owned();
    for cwd in [None, Some(cwd), Some(missing)] {
        for scan_limit in [0, 4097, usize::MAX] {
            let options = ListOptions {
                cwd: cwd.clone(),
                scan_limit,
            };
            assert_eq!(list(&client, &options), Err(AgentSessionError::Rejected));
        }
    }
}

#[test]
fn listing_accepts_scan_limit_endpoints() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "hello")]);
    for scan_limit in [1, 4096] {
        let options = ListOptions {
            cwd: Some(cwd.clone()),
            scan_limit,
        };
        let sessions = list(&client, &options).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_handle_id, SESSION);
    }
}

#[test]
fn listing_missing_project_directory_is_empty() {
    let (_directory, client, file, cwd) = fixture();
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    assert_eq!(
        list(
            &client,
            &ListOptions {
                cwd: Some(cwd),
                scan_limit: 10,
            },
        )
        .unwrap(),
        []
    );
}

#[cfg(unix)]
#[test]
fn listing_and_inspection_use_the_writer_symlink_spelling() {
    use std::os::unix::fs::symlink;

    let (directory, client, _file, _cwd) = fixture();
    let project = directory.path().join("project");
    let linked_project = directory.path().join("linked-project");
    std::fs::create_dir(&project).unwrap();
    symlink(&project, &linked_project).unwrap();
    let native_directory =
        project_directory(&directory.path().join("sessions"), &linked_project).unwrap();
    std::fs::create_dir_all(&native_directory).unwrap();
    let file = native_directory.join(format!("2026-10-18_{SESSION}.jsonl"));
    let native_cwd = linked_project.to_str().unwrap();
    let canonical_cwd = project.canonicalize().unwrap();
    write(
        &file,
        canonical_cwd.to_str().unwrap(),
        vec![user("u", Value::Null, "linked")],
    );
    let direct_directory =
        project_directory(&directory.path().join("sessions"), &canonical_cwd).unwrap();
    assert!(!direct_directory.exists());

    let sessions = list(
        &client,
        &ListOptions {
            cwd: Some(native_cwd.into()),
            scan_limit: 10,
        },
    )
    .unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].cwd, native_cwd);
    let history = inspect(&client, &handle(), native_cwd).unwrap();
    assert_eq!(history.entries[0].item["text"], "linked");
    assert_eq!(history.resume_metadata["cwd"], json!(canonical_cwd));
    let mut resumed_handle = handle();
    resumed_handle.native_handle = Some(json!(file));
    resumed_handle.metadata = Some(BTreeMap::from([
        ("cwd".into(), json!(canonical_cwd)),
        ("hasInput".into(), json!(true)),
    ]));
    assert_eq!(
        inspect(&client, &resumed_handle, canonical_cwd.to_str().unwrap())
            .unwrap()
            .entries[0]
            .item["text"],
        "linked"
    );
    let unfiltered = list(
        &client,
        &ListOptions {
            cwd: None,
            scan_limit: 10,
        },
    )
    .unwrap();
    assert_eq!(unfiltered.len(), 1);
    assert_eq!(unfiltered[0].cwd, native_cwd);
    assert_eq!(
        inspect(&client, &handle(), &unfiltered[0].cwd)
            .unwrap()
            .entries[0]
            .item["text"],
        "linked"
    );
    assert!(
        list(
            &client,
            &ListOptions {
                cwd: Some(canonical_cwd.to_str().unwrap().into()),
                scan_limit: 10,
            },
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        inspect(&client, &handle(), canonical_cwd.to_str().unwrap()).unwrap_err(),
        AgentSessionError::Unavailable
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_lookup_survives_canonical_placement_resume_and_a_renamed_file() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let (directory, mut client, _file, _cwd) = fixture();
    let project = directory.path().join("project");
    let linked = directory.path().join("linked-project");
    std::fs::create_dir(&project).unwrap();
    symlink(&project, &linked).unwrap();
    let folder = project_directory(&directory.path().join("sessions"), &linked).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("original.jsonl");
    let cwd = project.canonicalize().unwrap().to_str().unwrap().to_owned();
    write(&file, &cwd, vec![user("u", Value::Null, "linked")]);
    let history = inspect(&client, &handle(), linked.to_str().unwrap()).unwrap();
    let mut imported = handle();
    imported.metadata = Some(history.resume_metadata);
    assert_eq!(
        imported.metadata.as_ref().unwrap()["nativeCwd"],
        json!(linked)
    );
    let renamed = folder.join("renamed.jsonl");
    std::fs::rename(&file, &renamed).unwrap();

    let refreshed = inspect(&client, &imported, &cwd).unwrap();
    assert_eq!(refreshed.resume_metadata["sessionFile"], json!(renamed));
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("../tests/fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd,
        config: StoredAgentConfig::default(),
    };
    let mut session = client
        .resume_session(&imported, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let persisted = session.persistence().unwrap();
    assert_eq!(persisted.native_handle, Some(json!(renamed)));
    assert_eq!(
        persisted.metadata.as_ref().unwrap()["nativeCwd"],
        json!(linked)
    );
    session.close().await.unwrap();
    let moved = folder.join("moved-again.jsonl");
    std::fs::rename(&renamed, &moved).unwrap();
    assert_eq!(
        inspect(&client, &persisted, &spec.cwd)
            .unwrap()
            .resume_metadata["sessionFile"],
        json!(moved)
    );
    let mut no_filename = persisted;
    no_filename.native_handle = None;
    assert_eq!(
        inspect(&client, &no_filename, &spec.cwd)
            .unwrap()
            .resume_metadata["sessionFile"],
        json!(moved)
    );
    let mut resumed = client
        .resume_session(&no_filename, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.close().await.unwrap();

    no_filename
        .metadata
        .as_mut()
        .unwrap()
        .insert("nativeCwd".into(), json!(directory.path()));
    assert_eq!(
        inspect(&client, &no_filename, &spec.cwd).unwrap_err(),
        AgentSessionError::Rejected
    );
}

#[test]
fn listing_across_projects_skips_a_missing_directory() {
    let (directory, _client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "hello")]);
    let missing = directory.path().join("sessions/vanished");

    let paths =
        files_from_directories(vec![missing, file.parent().unwrap().to_owned()], None).unwrap();

    assert_eq!(paths, [file]);
}

#[test]
fn listing_skips_message_less_sessions_without_consuming_the_result_limit() {
    use std::time::{Duration, UNIX_EPOCH};

    let (_directory, client, file, cwd) = fixture();
    let valid = file.with_file_name("valid.jsonl");
    write(&valid, &cwd, vec![user("u", Value::Null, "real prompt")]);
    File::options()
        .write(true)
        .open(&valid)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();

    let startup = vec![
        json!({"type":"model_change","id":"model","parentId":null,
            "provider":"anthropic","modelId":"claude-sonnet-4-6"}),
        json!({"type":"thinking_level_change","id":"thinking","parentId":"model",
            "thinkingLevel":"off"}),
    ];
    let mut named = startup.clone();
    named.push(
        json!({"type":"session_info","id":"name","parentId":"thinking",
        "name":"Unused session"}),
    );
    let abandoned = vec![
        user("abandoned", Value::Null, "not on the current branch"),
        startup[0].clone(),
        startup[1].clone(),
    ];
    for entries in [vec![], startup, named, abandoned] {
        write(&file, &cwd, entries);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();

        let sessions = list(
            &client,
            &ListOptions {
                cwd: Some(cwd.clone()),
                scan_limit: 1,
            },
        )
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].first_prompt_preview.as_deref(),
            Some("real prompt")
        );
    }
}

#[test]
fn listing_keeps_messages_without_a_text_preview() {
    let (_directory, client, file, cwd) = fixture();
    let mut image_prompt = user("image", Value::Null, "");
    image_prompt["message"]["content"] =
        json!([{"type":"image","mimeType":"image/png","data":"aGVsbG8="}]);
    let shell = json!({"type":"message","id":"shell","parentId":null,"message":{
        "role":"bashExecution","command":"pwd","output":"/project","exitCode":0}});

    for message in [image_prompt, shell] {
        write(&file, &cwd, vec![message]);

        let sessions = list(
            &client,
            &ListOptions {
                cwd: Some(cwd.clone()),
                scan_limit: 1,
            },
        )
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].first_prompt_preview, None);
        assert_eq!(sessions[0].last_prompt_preview, None);
    }
}

#[test]
fn invalid_header_fields_consistently_report_rejected() {
    let (_directory, _client, file, cwd) = fixture();
    let valid = json!({"type":"session","version":3,"id":SESSION,"cwd":cwd,
        "timestamp":"2026-10-18T00:00:00Z"});
    for key in ["type", "version", "id", "cwd"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        std::fs::write(&file, format!("{missing}\n")).unwrap();
        assert_eq!(
            records(&file).unwrap_err(),
            AgentSessionError::Rejected,
            "missing {key}"
        );
        for invalid in [Value::Null, json!(""), json!(false), json!([])] {
            let mut header = valid.clone();
            header[key] = invalid;
            std::fs::write(&file, format!("{header}\n")).unwrap();
            assert_eq!(
                records(&file).unwrap_err(),
                AgentSessionError::Rejected,
                "invalid {key}"
            );
        }
    }
    for (key, invalid) in [
        ("type", json!("message")),
        ("version", json!(4)),
        ("id", json!("not-a-uuid")),
        ("cwd", json!("relative/path")),
    ] {
        let mut header = valid.clone();
        header[key] = invalid;
        std::fs::write(&file, format!("{header}\n")).unwrap();
        assert_eq!(
            records(&file).unwrap_err(),
            AgentSessionError::Rejected,
            "invalid {key}"
        );
    }
    std::fs::write(&file, format!("{valid}\n")).unwrap();
    assert_eq!(records(&file).unwrap(), vec![valid]);
}

#[test]
fn unreadable_or_malformed_header_records_still_report_failed() {
    let (_directory, _client, file, _cwd) = fixture();
    assert_eq!(records(&file).unwrap_err(), AgentSessionError::Failed);
    for data in ["", "{\"type\":", "not json\n"] {
        std::fs::write(&file, data).unwrap();
        assert_eq!(records(&file).unwrap_err(), AgentSessionError::Failed);
    }
}

#[test]
fn model_change_projects_root_selection_and_switch_after_assistant() {
    let (_directory, client, file, cwd) = fixture();
    // Pi 1.2's SessionManager.appendModelChange writes these exact field names,
    // including parentId: null when this is the first entry in the tree.
    let mut entries = vec![json!({"type":"model_change","id":"initial","parentId":null,
        "provider":"anthropic","modelId":"claude-sonnet-4-6"})];
    write(&file, &cwd, entries.clone());
    let history = inspect(&client, &handle(), &cwd).unwrap();
    assert_eq!(
        history.config.model.as_deref(),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert!(history.entries.is_empty());
    assert!(!history.active);

    entries.extend([
        user("u", json!("initial"), "hello"),
        assistant("a", "u", json!([{"type":"text","text":"hello"}])),
    ]);
    write(&file, &cwd, entries.clone());
    let answered = inspect(&client, &handle(), &cwd).unwrap();
    // Match Pi's context projection: subsequent assistant model facts also apply.
    assert_eq!(answered.config.model.as_deref(), Some("fixture/model"));

    entries.push(json!({"type":"model_change","id":"switch","parentId":"a",
        "provider":"openai","modelId":"gpt-4o"}));
    write(&file, &cwd, entries);
    let switched = inspect(&client, &handle(), &cwd).unwrap();
    assert_eq!(switched.config.model.as_deref(), Some("openai/gpt-4o"));
    assert_eq!(switched.entries, answered.entries);
    assert!(!switched.active);
}

#[test]
fn model_change_on_abandoned_branch_does_not_override_current_selection() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            json!({"type":"model_change","id":"initial","parentId":null,
            "provider":"anthropic","modelId":"claude-sonnet-4-6"}),
            json!({"type":"model_change","id":"current","parentId":"initial",
            "provider":"openai","modelId":"gpt-4o"}),
            json!({"type":"model_change","id":"abandoned","parentId":"initial",
            "provider":"fixture","modelId":"discarded-model"}),
            json!({"type":"session_info","id":"leaf","parentId":"current","name":"Current branch"}),
        ],
    );
    let history = inspect(&client, &handle(), &cwd).unwrap();
    assert_eq!(history.config.model.as_deref(), Some("openai/gpt-4o"));
    assert!(history.entries.is_empty());
}

#[test]
fn malformed_model_change_fields_and_missing_parent_are_rejected() {
    let (_directory, client, file, cwd) = fixture();
    let valid = json!({"type":"model_change","id":"model","parentId":null,
        "provider":"openai","modelId":"gpt-4o"});
    for key in ["provider", "modelId", "parentId"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        write(&file, &cwd, vec![missing]);
        assert_eq!(
            inspect(&client, &handle(), &cwd).unwrap_err(),
            AgentSessionError::Failed,
            "missing {key}"
        );
    }
    for key in ["provider", "modelId"] {
        for invalid in [Value::Null, json!(""), json!(42)] {
            let mut entry = valid.clone();
            entry[key] = invalid;
            write(&file, &cwd, vec![entry]);
            assert_eq!(
                inspect(&client, &handle(), &cwd).unwrap_err(),
                AgentSessionError::Failed,
                "invalid {key}"
            );
        }
    }
    let mut renamed = valid;
    let model = renamed.as_object_mut().unwrap().remove("modelId").unwrap();
    renamed["model"] = model;
    write(&file, &cwd, vec![renamed]);
    assert_eq!(
        inspect(&client, &handle(), &cwd).unwrap_err(),
        AgentSessionError::Failed
    );
}

#[test]
fn inspection_rejects_projected_models_that_cannot_be_resumed() {
    let (_directory, client, file, cwd) = fixture();
    let oversized_model = "m".repeat(512);
    let mut assistant_entry = assistant("a", "u", json!([]));
    assistant_entry["message"]["provider"] = json!("p");
    assistant_entry["message"]["model"] = json!(oversized_model);
    let histories = [
        vec![json!({"type":"model_change","id":"model","parentId":null,
            "provider":"p","modelId":"m".repeat(512)})],
        vec![user("u", Value::Null, "hello"), assistant_entry],
    ];

    for entries in histories {
        write(&file, &cwd, entries);
        assert_eq!(
            inspect(&client, &handle(), &cwd).unwrap_err(),
            AgentSessionError::Rejected
        );
    }
}

#[test]
fn tools_and_incomplete_turns_are_projected() {
    let (_directory, client, file, cwd) = fixture();
    let mut entries = vec![
        user("u", Value::Null, "run"),
        assistant(
            "a",
            "u",
            json!([
                {"type":"toolCall","id":"call","name":"bash","arguments":{"command":"pwd"}}
            ]),
        ),
        json!({"type":"message","id":"t","parentId":"a","message":{
        "role":"toolResult","toolCallId":"call","toolName":"bash","isError":false,
        "content":[{"type":"text","text":"/project"}]}}),
    ];
    write(&file, &cwd, entries.clone());
    assert!(inspect(&client, &handle(), &cwd).unwrap().active);
    entries.push(assistant(
        "done",
        "t",
        json!([{"type":"text","text":"done"}]),
    ));
    write(&file, &cwd, entries);
    let history = inspect(&client, &handle(), &cwd).unwrap();
    assert!(!history.active);
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["status"] == "completed")
    );
    write(&file, &cwd, vec![user("u", Value::Null, "pending")]);
    assert!(inspect(&client, &handle(), &cwd).unwrap().active);
}

#[test]
fn null_or_missing_message_content_is_normalized_like_pi() {
    let (_directory, client, file, cwd) = fixture();
    for remove_content in [false, true] {
        let mut empty_user = user("empty-user", Value::Null, "");
        let mut empty_assistant = assistant("empty-assistant", "empty-user", Value::Null);
        let mut empty_result = json!({"type":"message","id":"result","parentId":"call",
            "message":{"role":"toolResult","toolCallId":"tool","toolName":"fixture_tool",
                "isError":false,"content":null}});
        if remove_content {
            empty_user["message"]
                .as_object_mut()
                .unwrap()
                .remove("content");
            empty_assistant["message"]
                .as_object_mut()
                .unwrap()
                .remove("content");
            empty_result["message"]
                .as_object_mut()
                .unwrap()
                .remove("content");
        }
        write(
            &file,
            &cwd,
            vec![
                empty_user,
                empty_assistant,
                user("run", json!("empty-assistant"), "run"),
                assistant(
                    "call",
                    "run",
                    json!([{"type":"toolCall","id":"tool","name":"fixture_tool",
                        "arguments":{}}]),
                ),
                empty_result,
                assistant("done", "result", json!([])),
            ],
        );

        let history = inspect(&client, &handle(), &cwd).unwrap();
        let tool = history
            .entries
            .iter()
            .find(|entry| entry.item["type"] == "tool_call")
            .unwrap();

        assert!(!history.active);
        assert_eq!(tool.item["status"], "completed");
        assert_eq!(tool.item["detail"]["output"]["content"], json!([]));
    }
}

#[test]
fn oversized_assistant_block_is_split_during_inspection() {
    let (_directory, client, file, cwd) = fixture();
    let text = "x".repeat(120 * 1024);
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            assistant("a", "u", json!([{"type":"text","text":text}])),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let assistant_entries: Vec<_> = history
        .entries
        .iter()
        .filter(|entry| entry.item["type"] == "assistant_message")
        .collect();
    let projected: String = assistant_entries
        .iter()
        .map(|entry| entry.item["text"].as_str().unwrap())
        .collect();

    assert_eq!(assistant_entries.len(), 2);
    assert_eq!(projected, text);
}

#[test]
fn cumulative_live_text_budget_does_not_reject_history() {
    let (_directory, client, file, cwd) = fixture();
    let block = "x".repeat(96 * 1024);
    let reasoning = vec![json!({"type":"thinking","thinking":block}); 86];
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            assistant("a", "u", Value::Array(reasoning)),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let reasoning_entries: Vec<_> = history
        .entries
        .iter()
        .filter(|entry| entry.item["type"] == "reasoning")
        .collect();

    assert_eq!(reasoning_entries.len(), 86);
    assert!(
        reasoning_entries
            .iter()
            .all(|entry| entry.item["text"].as_str() == Some(block.as_str()))
    );
}

#[test]
fn inspection_caps_cumulative_projected_entries() {
    let (_directory, client, file, cwd) = fixture();
    let first_blocks = vec![json!({"type":"text","text":"x"}); 2047];
    let second_blocks = vec![json!({"type":"text","text":"x"}); 2048];
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            assistant("a1", "u", Value::Array(first_blocks.clone())),
            assistant("a2", "a1", Value::Array(second_blocks.clone())),
        ],
    );

    assert_eq!(
        inspect(&client, &handle(), &cwd).unwrap().entries.len(),
        MAX_HISTORY_ENTRIES
    );

    let mut overflowing_blocks = second_blocks;
    overflowing_blocks.push(json!({"type":"text","text":"x"}));
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            assistant("a1", "u", Value::Array(first_blocks)),
            assistant("a2", "a1", Value::Array(overflowing_blocks)),
        ],
    );

    assert_eq!(
        inspect(&client, &handle(), &cwd).unwrap_err(),
        AgentSessionError::Failed
    );
}

#[test]
fn tool_result_images_are_materialized_without_embedding_base64() {
    let (directory, mut client, file, cwd) = fixture();
    let image_directory = directory.path().join("images");
    client.images = crate::local::images::ImageStore::new(image_directory.clone());
    let image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "show image"),
            assistant(
                "a",
                "u",
                json!([{"type":"toolCall","id":"call","name":"image_tool","arguments":{}}]),
            ),
            json!({"type":"message","id":"t","parentId":"a","message":{
                "role":"toolResult","toolCallId":"call","toolName":"image_tool",
                "isError":false,"content":[{"type":"image","mimeType":"image/png","data":image}]}}),
            assistant("done", "t", json!([{"type":"text","text":"done"}])),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let serialized = serde_json::to_string(&history.entries).unwrap();
    let tool_index = history
        .entries
        .iter()
        .position(|entry| entry.item["type"] == "tool_call")
        .unwrap();
    let image_index = history
        .entries
        .iter()
        .position(|entry| {
            entry.item["text"]
                .as_str()
                .is_some_and(|text| text.contains("![Image]("))
        })
        .unwrap();

    assert!(!serialized.contains(image));
    assert!(tool_index < image_index);
    assert_eq!(std::fs::read_dir(image_directory).unwrap().count(), 1);
}

#[test]
fn tool_result_images_reconcile_without_conflicts_or_loss() {
    for second_image in [
        "https://example.com/second.png",
        "https://example.com/first.png",
    ] {
        let (_directory, client, file, cwd) = fixture();
        let first_image = "https://example.com/first.png";
        write(
            &file,
            &cwd,
            vec![
                user("u", Value::Null, "show images"),
                assistant(
                    "a",
                    "u",
                    json!([{"type":"toolCall","id":"call","name":"image_tool","arguments":{}}]),
                ),
                json!({"type":"message","id":"t","parentId":"a","message":{
                "role":"toolResult","toolCallId":"call","toolName":"image_tool",
                "isError":false,"content":[
                    {"type":"image","url":first_image},
                    {"type":"image","url":second_image}
                ]}}),
                assistant("done", "t", json!([{"type":"text","text":"done"}])),
            ],
        );

        let history = inspect(&client, &handle(), &cwd).unwrap();
        let timeline = crate::storage::timeline::Timeline::memory().unwrap();
        let epoch = timeline
            .reconcile("agent", PROVIDER, &history.entries)
            .unwrap();
        let refreshed = inspect(&client, &handle(), &cwd).unwrap();

        assert_eq!(
            timeline
                .reconcile("agent", PROVIDER, &refreshed.entries)
                .unwrap(),
            epoch
        );
        let rows = timeline.read("agent").unwrap().1;
        let images: Vec<_> = rows
            .iter()
            .filter_map(|row| row.entry.item["text"].as_str())
            .flat_map(str::lines)
            .filter(|line| line.starts_with("![Image]("))
            .collect();
        assert_eq!(
            images,
            [
                format!("![Image]({first_image})"),
                format!("![Image]({second_image})")
            ]
        );
    }
}

#[test]
fn empty_user_text_blocks_with_images_allow_inspection_and_refresh() {
    for edited in [false, true] {
        let (directory, mut client, file, cwd) = fixture();
        client.images = crate::local::images::ImageStore::new(directory.path().join("images"));
        let content = json!([
            {"type":"text","text":""},
            {"type":"image","mimeType":"image/png","data":"aGVsbG8="}
        ]);
        let mut input = user("u", Value::Null, "original user");
        if !edited {
            input["message"]["content"] = content.clone();
        }
        let mut entries = vec![
            input,
            assistant("a", "u", json!([{"type":"text","text":"done"}])),
        ];
        if edited {
            entries.push(json!({"type":"context_edit","id":"edit","parentId":"a",
                "targetId":"u","replacement":{"content":content}}));
        }
        write(&file, &cwd, entries);
        let original = std::fs::read(&file).unwrap();

        let history = inspect(&client, &handle(), &cwd).unwrap();

        assert!(!history.active);
        assert_eq!(history.entries.len(), 2);
        let input = &history.entries[0].item;
        assert_eq!(input["type"], "user_message");
        assert_eq!(input["text"], "");
        let sources = input["imageSources"].as_array().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            std::fs::read(sources[0].as_str().unwrap()).unwrap(),
            b"hello"
        );
        assert_eq!(history.entries[1].item["text"], "done");
        assert_eq!(
            history.entries,
            inspect(&client, &handle(), &cwd).unwrap().entries
        );
        assert_eq!(std::fs::read(&file).unwrap(), original);
    }
}

#[test]
fn image_only_user_context_edit_projects_a_preview_without_markdown_text() {
    let (directory, mut client, file, cwd) = fixture();
    client.images = crate::local::images::ImageStore::new(directory.path().join("images"));
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "stale user"),
            assistant("a", "u", json!([{"type":"text","text":"done"}])),
            json!({"type":"context_edit","id":"edit","parentId":"a","targetId":"u",
                "replacement":{"content":[{"type":"image","mimeType":"image/png",
                    "data":"aGVsbG8="}]}}),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert!(!history.active);
    let input = &history.entries[0].item;
    assert_eq!(input["type"], "user_message");
    assert_eq!(input["text"], "");
    let sources = input["imageSources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(
        std::fs::read(sources[0].as_str().unwrap()).unwrap(),
        b"hello"
    );
    assert_eq!(history.entries[1].item["text"], "done");
    assert_eq!(
        history.entries,
        inspect(&client, &handle(), &cwd).unwrap().entries
    );
}

#[test]
fn projects_images_compaction_extension_messages_and_manual_shell_commands() {
    let (directory, mut client, file, cwd) = fixture();
    client.images = crate::local::images::ImageStore::new(directory.path().join("images"));
    let mut input = user("u", Value::Null, "");
    input["message"]["content"] = json!([
        {"type":"text","text":"image"},
        {"type":"image","mimeType":"image/png","data":"aGVsbG8="}
    ]);
    write(
        &file,
        &cwd,
        vec![
            input,
            assistant("a", "u", json!([{"type":"text","text":"done"}])),
            json!({"type":"compaction","id":"c","parentId":"a","summary":"summary",
                "firstKeptEntryId":"u","tokensBefore":1024}),
            json!({"type":"custom_message","id":"hidden","parentId":"c","display":false,"content":"hidden"}),
            json!({"type":"custom_message","id":"visible","parentId":"hidden","display":true,"content":"visible"}),
            json!({"type":"branch_summary","id":"branch","parentId":"visible","summary":"branch summary"}),
            json!({"type":"message","id":"shell","parentId":"branch","message":{
            "role":"bashExecution","command":"pwd","output":"/project","exitCode":0}}),
        ],
    );
    let history = inspect(&client, &handle(), &cwd).unwrap();
    assert!(!history.active);
    let input = history
        .entries
        .iter()
        .find(|entry| entry.item["type"] == "user_message")
        .unwrap();
    assert_eq!(input.item["text"], "image");
    let sources = input.item["imageSources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(
        std::fs::read(sources[0].as_str().unwrap()).unwrap(),
        b"hello"
    );
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["type"] == "compaction")
    );
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "visible")
    );
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "hidden")
    );
    assert_eq!(history.entries.last().unwrap().item["status"], "completed");
    assert_eq!(
        history.entries,
        inspect(&client, &handle(), &cwd).unwrap().entries
    );
}

#[test]
fn compaction_replaces_older_history_with_summary_and_retained_tail() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            json!({"type":"message","id":"system","parentId":null,
                "message":{"role":"system","content":"initial instructions"}}),
            user("old-user", json!("system"), "omitted user"),
            assistant(
                "old-assistant",
                "old-user",
                json!([{"type":"text","text":"omitted assistant"}]),
            ),
            json!({"type":"message","id":"kept-system","parentId":"old-assistant",
                "message":{"role":"system","content":"checkpoint instructions"}}),
            user("kept-user", json!("kept-system"), "retained user"),
            assistant(
                "kept-assistant",
                "kept-user",
                json!([{"type":"text","text":"retained assistant"}]),
            ),
            json!({"type":"compaction","id":"compact","parentId":"kept-assistant",
                "summary":"compacted summary","firstKeptEntryId":"kept-system",
                "tokensBefore":50_000}),
            user("new-user", json!("compact"), "new user"),
            assistant(
                "new-assistant",
                "new-user",
                json!([{"type":"text","text":"new assistant"}]),
            ),
        ],
    );

    let stored = records(&file).unwrap();
    let selected_branch = branch(&stored).unwrap();
    let context_ids: Vec<_> = context_branch(&selected_branch)
        .unwrap()
        .into_iter()
        .map(|record| record["id"].as_str().unwrap())
        .collect();
    let history = inspect(&client, &handle(), &cwd).unwrap();
    let visible_text: Vec<_> = history
        .entries
        .iter()
        .filter_map(|entry| entry.item["text"].as_str())
        .collect();

    assert_eq!(
        context_ids,
        [
            "compact",
            "kept-user",
            "kept-assistant",
            "new-user",
            "new-assistant"
        ]
    );
    assert_eq!(history.entries[0].item["type"], "compaction");
    assert_eq!(
        visible_text,
        [
            "compacted summary",
            "retained user",
            "retained assistant",
            "new user",
            "new assistant"
        ]
    );
    assert!(!visible_text.contains(&"omitted user"));
    assert!(!visible_text.contains(&"omitted assistant"));
    assert!(!visible_text.contains(&"checkpoint instructions"));
    assert_eq!(history.config.model.as_deref(), Some("fixture/model"));
    assert!(!history.active);
}

#[test]
fn newest_compaction_suppresses_retained_older_checkpoint() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("old-user", Value::Null, "old user"),
            assistant(
                "old-assistant",
                "old-user",
                json!([{"type":"text","text":"old assistant"}]),
            ),
            json!({"type":"compaction","id":"old-compaction","parentId":"old-assistant",
                "summary":"obsolete summary","firstKeptEntryId":"old-user",
                "tokensBefore":10_000}),
            user("kept-user", json!("old-compaction"), "retained user"),
            assistant(
                "kept-assistant",
                "kept-user",
                json!([{"type":"text","text":"retained assistant"}]),
            ),
            json!({"type":"compaction","id":"new-compaction","parentId":"kept-assistant",
                "summary":"latest summary","firstKeptEntryId":"old-user",
                "tokensBefore":20_000}),
            user("new-user", json!("new-compaction"), "new user"),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let compactions: Vec<_> = history
        .entries
        .iter()
        .filter(|entry| entry.item["type"] == "compaction")
        .collect();
    let visible_text: Vec<_> = history
        .entries
        .iter()
        .filter_map(|entry| entry.item["text"].as_str())
        .collect();

    assert_eq!(compactions.len(), 1);
    assert_eq!(
        compactions[0].item["compactionId"],
        "native:pi:compaction:new-compaction"
    );
    assert_eq!(
        visible_text,
        [
            "latest summary",
            "old user",
            "old assistant",
            "retained user",
            "retained assistant",
            "new user"
        ]
    );
    assert!(!visible_text.contains(&"obsolete summary"));
}

#[test]
fn context_edits_replace_or_omit_model_visible_content() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "stale user"),
            assistant("a", "u", json!([{"type":"text","text":"stale assistant"}])),
            json!({"type":"custom_message","id":"custom","parentId":"a",
                "customType":"fixture","display":true,"content":"stale custom"}),
            json!({"type":"context_edit","id":"edit-user","parentId":"custom",
                "targetId":"u","replacement":{"content":"edited user"}}),
            json!({"type":"context_edit","id":"edit-user-again","parentId":"edit-user",
                "targetId":"u","replacement":{"content":"latest user"}}),
            json!({"type":"context_edit","id":"edit-assistant","parentId":"edit-user-again",
                "targetId":"a","replacement":null}),
            json!({"type":"context_edit","id":"edit-custom","parentId":"edit-assistant",
            "targetId":"custom","replacement":{"content":[
                {"type":"text","text":"edited custom"}
            ]}}),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let visible_text: Vec<_> = history
        .entries
        .iter()
        .filter_map(|entry| entry.item["text"].as_str())
        .collect();

    assert_eq!(visible_text, ["latest user", "edited custom"]);
    assert!(!history.active);
}

#[test]
fn omitted_final_assistant_settles_the_active_turn() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            assistant("a", "u", json!([{"type":"text","text":"hidden reply"}])),
            json!({"type":"context_edit","id":"edit","parentId":"a",
                "targetId":"a","replacement":null}),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert!(!history.active);
    assert!(
        history
            .entries
            .iter()
            .all(|entry| { entry.item["text"].as_str() != Some("hidden reply") })
    );
}

#[test]
fn context_edits_replace_tool_calls_in_content_order() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "run"),
            assistant(
                "a",
                "u",
                json!([
                    {"type":"toolCall","id":"stale-call","name":"bash",
                        "arguments":{"command":"false"}}
                ]),
            ),
            json!({"type":"message","id":"t","parentId":"a","message":{
                "role":"toolResult","toolCallId":"edited-call","toolName":"bash","isError":false,
                "content":[{"type":"text","text":"stale output"}]}}),
            json!({"type":"context_edit","id":"edit-assistant","parentId":"t",
            "targetId":"a","replacement":{"content":[
                {"type":"text","text":"before tool"},
                {"type":"toolCall","id":"edited-call","name":"bash",
                    "arguments":{"command":"pwd"}},
                {"type":"thinking","thinking":"after tool"}
            ]}}),
            json!({"type":"context_edit","id":"edit-tool","parentId":"edit-assistant",
                "targetId":"t","replacement":{"content":"edited output"}}),
            assistant("done", "edit-tool", json!([{"type":"text","text":"done"}])),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();
    let tool = history
        .entries
        .iter()
        .find(|entry| entry.item["type"] == "tool_call")
        .unwrap();
    let before_index = history
        .entries
        .iter()
        .position(|entry| entry.item["text"] == "before tool")
        .unwrap();
    let tool_index = history
        .entries
        .iter()
        .position(|entry| entry.item["type"] == "tool_call")
        .unwrap();
    let after_index = history
        .entries
        .iter()
        .position(|entry| entry.item["text"] == "after tool")
        .unwrap();

    assert!(!history.active);
    assert!(before_index < tool_index && tool_index < after_index);
    assert_eq!(tool.item["detail"]["command"], "pwd");
    assert_eq!(tool.item["detail"]["output"], "edited output");
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["detail"]["command"] == "false")
    );
}

#[test]
fn omitted_tool_result_still_completes_source_lifecycle() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "run"),
            assistant(
                "a",
                "u",
                json!([
                    {"type":"toolCall","id":"call","name":"bash",
                        "arguments":{"command":"pwd"}}
                ]),
            ),
            json!({"type":"message","id":"t","parentId":"a","message":{
                "role":"toolResult","toolCallId":"call","toolName":"bash","isError":false,
                "content":[{"type":"text","text":"/project"}]}}),
            json!({"type":"context_edit","id":"edit","parentId":"t",
                "targetId":"t","replacement":null}),
            assistant("done", "edit", json!([{"type":"text","text":"done"}])),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert!(!history.active);
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "done")
    );
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["status"] == "completed")
    );
}

#[test]
fn omitted_tool_call_rejects_retained_result() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "run"),
            assistant(
                "a",
                "u",
                json!([
                    {"type":"toolCall","id":"call","name":"bash",
                        "arguments":{"command":"pwd"}}
                ]),
            ),
            json!({"type":"message","id":"t","parentId":"a","message":{
                "role":"toolResult","toolCallId":"call","toolName":"bash","isError":false,
                "content":[{"type":"text","text":"/project"}]}}),
            json!({"type":"context_edit","id":"edit","parentId":"t",
                "targetId":"a","replacement":null}),
            assistant("done", "edit", json!([{"type":"text","text":"done"}])),
        ],
    );

    assert_eq!(
        inspect(&client, &handle(), &cwd).unwrap_err(),
        AgentSessionError::Failed
    );
}

#[test]
fn malformed_context_edit_replacement_is_rejected() {
    let (_directory, client, file, cwd) = fixture();
    for replacement in [json!({}), json!({"content":false}), json!("invalid")] {
        write(
            &file,
            &cwd,
            vec![
                user("u", Value::Null, "user"),
                json!({"type":"context_edit","id":"edit","parentId":"u",
                    "targetId":"u","replacement":replacement}),
            ],
        );

        assert_eq!(
            inspect(&client, &handle(), &cwd).unwrap_err(),
            AgentSessionError::Failed
        );
    }
}

#[test]
fn projects_visible_legacy_custom_role_messages() {
    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hello"),
            json!({"type":"message","id":"replaced","parentId":"u","message":{
                "role":"custom","customType":"fixture","display":true,
                "content":"stale legacy extension message"}}),
            json!({"type":"message","id":"omitted","parentId":"replaced","message":{
                "role":"custom","customType":"fixture","display":true,
                "content":"omitted legacy extension message"}}),
            json!({"type":"message","id":"hidden","parentId":"omitted","message":{
                "role":"custom","customType":"fixture","display":false,
                "content":"hidden legacy extension message"}}),
            json!({"type":"context_edit","id":"replace","parentId":"hidden",
                "targetId":"replaced","replacement":{"content":"edited legacy extension message"}}),
            json!({"type":"context_edit","id":"omit","parentId":"replace",
                "targetId":"omitted","replacement":null}),
            assistant("a", "omit", json!([{"type":"text","text":"done"}])),
        ],
    );

    let history = inspect(&client, &handle(), &cwd).unwrap();

    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "edited legacy extension message")
    );
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "stale legacy extension message")
    );
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "omitted legacy extension message")
    );
    assert!(
        !history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "hidden legacy extension message")
    );
}

#[test]
fn listing_skips_corrupt_files_without_consuming_valid_session_limit() {
    use std::io::Write;
    use std::time::{Duration, UNIX_EPOCH};

    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "valid session")]);
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    for index in 0..20 {
        let corrupt = file.with_file_name(format!("corrupt-{index}.jsonl"));
        // Keep the cwd-bearing header valid, as when a writer crashes mid-entry.
        write(&corrupt, &cwd, vec![]);
        let mut writer = File::options().append(true).open(&corrupt).unwrap();
        writer.write_all(b"{\"type\":\"message\"").unwrap();
        writer
            .set_modified(UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();
    }
    let paths = files(&client, None, None).unwrap();
    assert_eq!(paths.len(), 21);
    assert_eq!(paths.last(), Some(&file));

    for filter in [Some(cwd), None] {
        let sessions = list(
            &client,
            &ListOptions {
                cwd: filter,
                scan_limit: 20,
            },
        )
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_handle_id, SESSION);
        assert_eq!(
            sessions[0].first_prompt_preview.as_deref(),
            Some("valid session")
        );
    }
}

#[test]
fn listing_stops_after_the_candidate_work_budget() {
    use std::time::{Duration, UNIX_EPOCH};

    let (_directory, client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![user("u", Value::Null, "older valid session")],
    );
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    for index in 0..LIST_CANDIDATE_MULTIPLIER {
        let corrupt = file.with_file_name(format!("newer-corrupt-{index}.jsonl"));
        std::fs::write(&corrupt, b"not json\n").unwrap();
        File::options()
            .write(true)
            .open(corrupt)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(2 + index as u64))
            .unwrap();
    }

    let sessions = list(
        &client,
        &ListOptions {
            cwd: Some(cwd),
            scan_limit: 1,
        },
    )
    .unwrap();

    assert!(sessions.is_empty());
}

#[test]
fn listing_stops_at_an_expired_deadline() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "session")]);

    let sessions = list_before(
        &client,
        &ListOptions {
            cwd: Some(cwd),
            scan_limit: 1,
        },
        Instant::now(),
    )
    .unwrap();

    assert!(sessions.is_empty());
}

#[test]
fn listing_reader_reserves_each_file_from_the_byte_budget() {
    let (directory, _client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "session")]);
    let file_size = file.metadata().unwrap().len();
    let mut insufficient = file_size - 1;

    assert!(
        HistoryReader::open_with_budget(&file, &mut insufficient)
            .unwrap()
            .is_none()
    );
    assert_eq!(insufficient, file_size - 1);

    let mut exact = file_size;
    let opened = HistoryReader::open_with_budget(&file, &mut exact)
        .unwrap()
        .unwrap();
    assert_eq!(exact, 0);
    assert_eq!(opened.records().unwrap().len(), 2);

    let corrupt = directory.path().join("corrupt.jsonl");
    std::fs::write(&corrupt, b"not json\n").unwrap();
    let mut corrupt_budget = corrupt.metadata().unwrap().len();
    assert!(HistoryReader::open_with_budget(&corrupt, &mut corrupt_budget).is_err());
    assert_eq!(corrupt_budget, 0);
}

#[test]
fn listing_limits_distinct_valid_sessions_after_skipping_duplicates() {
    use std::time::{Duration, UNIX_EPOCH};

    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "session")]);
    let template = std::fs::read_to_string(&file).unwrap();
    for index in 0..3 {
        let path = file.with_file_name(format!("valid-{index}.jsonl"));
        let id = format!("019a0000-0000-7000-8000-{index:012}");
        std::fs::write(&path, template.replace(SESSION, &id)).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(10 - index))
            .unwrap();
    }
    // This newer duplicate must not prevent the next distinct session being returned.
    std::fs::write(
        &file,
        template.replace(SESSION, "019a0000-0000-7000-8000-000000000000"),
    )
    .unwrap();
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(11))
        .unwrap();

    for limit in [1, 2] {
        let sessions = list(
            &client,
            &ListOptions {
                cwd: Some(cwd.clone()),
                scan_limit: limit,
            },
        )
        .unwrap();
        assert_eq!(sessions.len(), limit);
        for (index, session) in sessions.iter().enumerate() {
            assert_eq!(
                session.provider_handle_id,
                format!("019a0000-0000-7000-8000-{index:012}")
            );
        }
    }
}

#[test]
fn listing_is_bounded_and_filters_directories_and_preview_text() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, &"文".repeat(600))]);
    let options = ListOptions {
        cwd: Some(cwd.clone()),
        scan_limit: 1,
    };
    let entries = list(&client, &options).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]
            .first_prompt_preview
            .as_ref()
            .unwrap()
            .chars()
            .count(),
        500
    );
    assert!(
        list(
            &client,
            &ListOptions {
                cwd: Some("/absent".into()),
                ..options
            }
        )
        .unwrap()
        .is_empty()
    );
    let mut unsupported = std::fs::read_to_string(&file).unwrap();
    unsupported = unsupported.replace("\"version\":3", "\"version\":4");
    std::fs::write(&file, unsupported).unwrap();
    assert!(inspect(&client, &handle(), &cwd).is_err());
}

#[test]
fn listing_filters_before_scan_limit_and_accepts_equivalent_directory_paths() {
    let (directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "wanted")]);
    let other_cwd = directory.path().join("other-project");
    std::fs::create_dir(&other_cwd).unwrap();
    let unrelated_directory =
        project_directory(&directory.path().join("sessions"), &other_cwd).unwrap();
    std::fs::create_dir_all(&unrelated_directory).unwrap();
    let unrelated = unrelated_directory.join("unrelated.jsonl");
    write(&unrelated, other_cwd.to_str().unwrap(), vec![]);
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
        .unwrap();
    File::options()
        .write(true)
        .open(&unrelated)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(2))
        .unwrap();
    assert_eq!(files(&client, None, None).unwrap()[0], unrelated);
    assert_eq!(
        files(&client, Some(Path::new(&cwd)), None).unwrap(),
        vec![file.clone()]
    );
    let equivalent_cwd = Path::new(&cwd).join(".");
    let equivalent_directory =
        project_directory(&directory.path().join("sessions"), &equivalent_cwd).unwrap();
    std::fs::create_dir_all(&equivalent_directory).unwrap();
    let equivalent_file = equivalent_directory.join(file.file_name().unwrap());
    std::fs::rename(&file, equivalent_file).unwrap();

    let sessions = list(
        &client,
        &ListOptions {
            cwd: Some(equivalent_cwd.to_str().unwrap().into()),
            scan_limit: 1,
        },
    )
    .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].cwd, equivalent_cwd.to_str().unwrap());
    assert_eq!(sessions[0].first_prompt_preview.as_deref(), Some("wanted"));
}

#[test]
fn opened_history_rejects_changes_after_reading_its_header() {
    let (_directory, _client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "original")]);
    let opened = HistoryReader::open(&file).unwrap();
    write(
        &file,
        &cwd,
        vec![user("u", Value::Null, "changed and longer")],
    );
    assert_eq!(opened.records().unwrap_err(), AgentSessionError::Failed);
}

#[cfg(unix)]
#[test]
fn opened_history_keeps_header_and_body_from_the_same_file_when_path_is_replaced() {
    let (directory, _client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "original")]);
    let opened = HistoryReader::open(&file).unwrap();
    std::fs::rename(&file, directory.path().join("original.jsonl")).unwrap();
    let replacement_cwd = directory.path().join("replacement");
    std::fs::create_dir(&replacement_cwd).unwrap();
    write(
        &file,
        replacement_cwd.to_str().unwrap(),
        vec![user("u", Value::Null, "replacement")],
    );

    let original = opened.records().unwrap();
    assert_eq!(original[0]["cwd"], cwd);
    assert_eq!(original[1]["message"]["content"], "original");
    assert_eq!(
        records(&file).unwrap()[0]["cwd"],
        replacement_cwd.to_str().unwrap()
    );
}

#[test]
fn rejects_foreign_cwd_identity_malformed_trees_and_partial_records() {
    let (_directory, client, file, cwd) = fixture();
    write(&file, &cwd, vec![user("u", Value::Null, "hi")]);
    assert!(inspect(&client, &handle(), "/missing-directory").is_err());
    let mut foreign = handle();
    foreign.provider = "claude".into();
    assert!(inspect(&client, &foreign, &cwd).is_err());
    for entries in [
        vec![user("u", json!("missing"), "hi")],
        vec![user("u", json!("u"), "hi")],
        vec![
            user("u", Value::Null, "hi"),
            user("u", Value::Null, "duplicate"),
        ],
    ] {
        write(&file, &cwd, entries);
        assert!(inspect(&client, &handle(), &cwd).is_err());
    }
    std::fs::write(&file, "{\"type\":").unwrap();
    assert!(inspect(&client, &handle(), &cwd).is_err());
    assert!(
        list(
            &client,
            &ListOptions {
                cwd: None,
                scan_limit: 10
            }
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn accepts_the_record_limit_and_rejects_records_above_it() {
    let (_directory, _client, file, cwd) = fixture();
    write_history_with_record_count(&file, &cwd, MAX_HISTORY_RECORDS);
    assert_eq!(records(&file).unwrap().len(), MAX_HISTORY_RECORDS + 1);

    write_history_with_record_count(&file, &cwd, MAX_HISTORY_RECORDS + 1);
    assert_eq!(records(&file), Err(AgentSessionError::Failed));
}

#[cfg(unix)]
#[test]
fn ignores_symlinks_and_missing_store_and_rejects_large_files() {
    let (directory, client, file, cwd) = fixture();
    let target = directory.path().join("outside.jsonl");
    write(&target, &cwd, vec![]);
    std::os::unix::fs::symlink(&target, &file).unwrap();
    assert!(
        list(
            &client,
            &ListOptions {
                cwd: None,
                scan_limit: 10
            }
        )
        .unwrap()
        .is_empty()
    );
    assert!(records(&file).is_err());
    File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_len(MAX_HISTORY + 1)
        .unwrap();
    assert!(records(&target).is_err());
    let mut absent = client;
    absent.session_environment =
        SessionEnvironment::isolated(directory.path(), &directory.path().join("absent"));
    assert!(files(&absent, None, None).unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn imported_session_refresh_uses_the_native_handle_after_resume() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, mut client, file, cwd) = fixture();
    write(
        &file,
        &cwd,
        vec![
            user("u", Value::Null, "hi"),
            assistant("a", "u", json!([{"type":"text","text":"hello"}])),
        ],
    );
    let history = inspect(&client, &handle(), &cwd).unwrap();
    let program = Path::new(&cwd).join("pi");
    std::fs::write(&program, include_str!("../tests/fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    let mut imported = handle();
    imported.metadata = Some(history.resume_metadata);
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd,
        config: history.config,
    };
    let mut session = client
        .resume_session(&imported, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let resumed_handle = session.persistence().unwrap();
    assert_eq!(resumed_handle.session_id, SESSION);
    assert_eq!(resumed_handle.native_handle, Some(json!(file)));
    assert!(
        resumed_handle
            .metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.contains_key("sessionFile"))
    );
    session.close().await.unwrap();
    client.session_environment.session_directory =
        Some(directory.path().join("new-session-store").into_os_string());

    let refreshed = inspect(&client, &resumed_handle, &spec.cwd).unwrap();

    assert_eq!(refreshed.entries[0].item["text"], "hi");
    assert_eq!(refreshed.resume_metadata["sessionFile"], json!(file));
}
