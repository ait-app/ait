use super::*;

/// A journal of `entries`, already the active branch, for session `id`.
fn journal(path: PathBuf, entries: Vec<Value>) -> Journal {
    Journal {
        path,
        header: json!({"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        title: None,
        entries,
        identity: "id".to_owned(),
        parent: None,
    }
}

/// Read `path` as [`Lookup::load`] does, without its identity checks.
fn read_journal(path: PathBuf) -> Journal {
    let snapshot = reader::read(&path).unwrap();
    let identity = snapshot.header["id"].as_str().unwrap().to_owned();
    Journal {
        path,
        header: snapshot.header,
        title: snapshot.title,
        entries: snapshot.entries,
        identity,
        parent: None,
    }
}

fn handle(id: &str, metadata: Option<BTreeMap<String, Value>>) -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: "omp".into(),
        session_id: id.into(),
        native_handle: None,
        metadata,
    }
}

fn record(id: &str, parent: Option<&str>, text: &str) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"timestamp":"2026-10-21T00:00:00Z",
        "message":{"role":"user","timestamp":1000,"content":text}})
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

fn timeline(stream: &mut Stream) -> Vec<provider_core::protocol::timeline::NativeItem> {
    stream
        .events
        .drain(..)
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry),
            _ => None,
        })
        .collect()
}

#[test]
fn title_slots_and_off_branch_usage_preserve_the_active_transcript() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    // OMP 18.18.8 writes its padded title slot before the header and rewrites it in place.
    let lines = [
        json!({"type":"title","v":1,"title":"Native title","source":"user","pad":"  "}),
        json!({"type":"session","version":3,"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        record("first", None, "kept"),
        record("old", Some("first"), "abandoned"),
        json!({"type":"model_usage","id":"usage","parentId":"old","activeLeafId":"first","timestamp":"2026-10-21T00:00:01Z"}),
    ]
    .map(|line| line.to_string());
    write_lines(&path, &lines);

    let (history, _) = read_journal(path)
        .project(&OmpClient::new("unused".into()), &handle("id", None))
        .unwrap();

    assert_eq!(history.descriptor.title.as_deref(), Some("Native title"));
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].item["text"], "kept");
}

/// The OMP 18.18.8 records of one session file, with the shapes recorded by the B7 probes.
fn mapped_session() -> Vec<String> {
    let message = |id: &str, parent: &str, time: i64, message: Value| {
        let mut message = message;
        message["timestamp"] = json!(time);
        json!({"type":"message","id":id,"parentId":parent,
            "timestamp":"2026-10-21T00:00:01Z","message":message})
        .to_string()
    };
    vec![
        json!({"type":"title","v":1,"title":"","updatedAt":"2026-10-21T00:00:00Z"}).to_string(),
        json!({"type":"session","version":3,"id":"id","cwd":"/tmp",
            "timestamp":"2026-10-21T00:00:00Z"})
        .to_string(),
        json!({"type":"model_change","id":"m","parentId":null,
            "timestamp":"2026-10-21T00:00:00Z","model":"test/model"})
        .to_string(),
        json!({"type":"session_init","id":"init","parentId":"m",
            "timestamp":"2026-10-21T00:00:00Z","systemPrompt":["prompt"]})
        .to_string(),
        json!({"type":"custom","customType":"date-cwd-reminder","id":"r","parentId":"init",
            "timestamp":"2026-10-21T00:00:00Z","data":{}})
        .to_string(),
        message(
            "u",
            "r",
            1000,
            json!({"role":"user","content":[{"type":"text","text":"go"}]}),
        ),
        "{\"type\":\"message\",\"id\":\"broken\"".to_owned(),
        message(
            "dev",
            "u",
            1010,
            json!({"role":"developer","content":[{"type":"text",
            "text":"<system-reminder>\nYou stopped with 2 incomplete todo item(s)\n</system-reminder>"}]}),
        ),
        message(
            "note",
            "dev",
            1020,
            json!({"role":"developer","content":[{"type":"text",
            "text":"Visible developer note"}]}),
        ),
        message(
            "sys",
            "note",
            1030,
            json!({"role":"system","content":"hidden"}),
        ),
        message(
            "file",
            "sys",
            1040,
            json!({"role":"fileMention","content":"file text"}),
        ),
        message(
            "bash",
            "file",
            1050,
            json!({"role":"bashExecution","command":"echo hi",
            "output":"hi\n","exitCode":0,"cancelled":false,"truncated":false}),
        ),
        json!({"type":"compaction","id":"c","parentId":"bash","timestamp":"2026-10-21T00:00:02Z",
            "summary":"done","firstKeptEntryId":"u","tokensBefore":100_010,"tokensAfter":8708})
        .to_string(),
        json!({"type":"service_tier_change","id":"tier","parentId":"c",
            "timestamp":"2026-10-21T00:00:02Z","serviceTier":null})
        .to_string(),
        json!({"type":"vendor_entry","id":"v","parentId":"tier",
            "timestamp":"2026-10-21T00:00:03Z","text":"vendor text"})
        .to_string(),
        message(
            "call",
            "v",
            1060,
            json!({"role":"assistant","provider":"test","model":"model",
            "stopReason":"toolUse","content":[{"type":"toolCall","id":"run","name":"bash",
                "arguments":{"command":"sleep 9"}}]}),
        ),
        // An abandoned leaf: its parent is not the last entry, and nothing names it.
        message(
            "other",
            "u",
            1070,
            json!({"role":"user","content":"abandoned"}),
        ),
        // The active leaf follows, so the last entry that is no one's parent is `call`'s child.
        message(
            "tail",
            "call",
            1080,
            json!({"role":"user","content":"continue"}),
        ),
        "{\"type\":\"message\",\"id\":\"partial\",\"parentId\":\"tail\",\"mess".to_owned(),
    ]
}

#[test]
fn history_maps_omp_entries_as_paseo_does_and_tolerates_damaged_lines() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.jsonl");
    write_lines(&path, &mapped_session());

    let (history, _) = read_journal(path)
        .project(&OmpClient::new("unused".into()), &handle("id", None))
        .unwrap();

    let items: Vec<_> = history.entries.iter().map(|entry| &entry.item).collect();
    assert_eq!(items.len(), 7, "{items:#?}");
    assert_eq!(items[0]["text"], "go");
    assert_eq!(
        items[1]["detail"]["text"],
        "[developer] Visible developer note"
    );
    assert_eq!(
        items[2],
        &json!({"type":"tool_call","callId":"omp-bash-1050","name":"bash","status":"completed",
            "detail":{"type":"shell","command":"echo hi","output":"hi\n","exitCode":0},
            "error":null})
    );
    assert_eq!(
        items[3],
        &json!({"type":"compaction","status":"completed","preTokens":100_010})
    );
    assert_eq!(history.entries[3].key, "native:omp:id:compaction:c");
    assert_eq!(items[4]["detail"]["text"], "[vendor_entry] vendor text");
    // A call without a result on the branch is still running.
    assert_eq!(items[5]["status"], "running");
    assert_eq!(items[5]["detail"]["command"], "sleep 9");
    assert_eq!(history.entries[5].key, "native:omp:id:tool:run");
    assert_eq!(items[6]["text"], "continue");
    assert!(history.active);
    assert_eq!(history.config.model.as_deref(), Some(r#"["test","model"]"#));
}

#[test]
fn live_messages_show_the_rows_history_shows_for_the_same_roles() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.jsonl");
    write_lines(&path, &mapped_session());
    let journal = read_journal(path);
    let (history, _) = journal
        .project(&OmpClient::new("unused".into()), &handle("id", None))
        .unwrap();
    let mut live = Stream::new("id".into(), provider_native::images::ImageStore::default());

    for entry in &journal.entries {
        if entry["type"] == "message" && entry["message"]["role"] != "user" {
            live.consume(&json!({"type":"message_end","message":entry["message"]}))
                .unwrap();
        }
    }

    // The developer note and the shell row; reminders, system and file mentions show nothing.
    assert_eq!(timeline(&mut live), history.entries[1..3]);
}

#[test]
fn background_results_are_the_same_notification_in_live_output_and_history() {
    let root = tempfile::tempdir().unwrap();
    // OMP 18.18.8 records a settled background job as a `custom_message` entry.
    let result = json!({"type":"custom_message","id":"background","parentId":"user",
        "timestamp":"2026-10-21T00:00:01Z","customType":"async-result","display":true,
        "attribution":"agent",
        "content":"<system-notice>\nBackground job bg_4 (make) has completed. Resume your work \
            using the result below.\ncommand output\n</system-notice>",
        "details":{"jobs":[{"jobId":"bg_4","status":"completed","type":"bash","label":"make"}]}});
    let journal = journal(
        root.path().join("history.jsonl"),
        vec![record("user", None, "request"), result.clone()],
    );
    let (history, _) = journal
        .project(&OmpClient::new("missing".into()), &handle("id", None))
        .unwrap();
    let mut live = Stream::new("id".into(), provider_native::images::ImageStore::default());
    live.message(&record_message(&result).unwrap()).unwrap();
    let AgentTurnEvent::Timeline(entry) = live.events.pop_front().unwrap() else {
        panic!("expected background result");
    };
    assert_eq!(history.entries.len(), 2);
    assert_eq!(history.entries[1], entry);
    assert_eq!(entry.item["type"], "notification");
    assert_eq!(
        entry.item["message"],
        "Background job bg_4 (make) has completed. Resume your work using the result below.\n\
            command output"
    );

    // A timeline that recorded the former background card is replaced by the notification.
    let timeline =
        provider::storage::timeline::Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut legacy = history.entries.clone();
    legacy[1].item = json!({"type":"tool_call","callId":"background:bg_4","name":"Background job",
        "status":"completed","error":null,
        "detail":{"type":"unknown","input":"make","output":result["content"]}});
    let (old_epoch, _) =
        provider::testing::append_timeline(&timeline, "agent", "omp", &legacy).unwrap();
    let replacement =
        provider::testing::reconcile_timeline(&timeline, "agent", "omp", &history.entries).unwrap();
    assert_ne!(replacement, old_epoch);
    let (_, restored) = provider::testing::timeline_entries(&timeline, "agent").unwrap();
    assert_eq!(restored[1].item, entry.item);
}

const IMAGE_SESSION: &str = "00000000-0000-4000-8000-000000000001";

fn image_session(root: &Path, image: &Value) -> PathBuf {
    let sessions = root.join("agent/sessions/project");
    let path = sessions.join(format!("2026-10-21T00-00-00-000Z_{IMAGE_SESSION}.jsonl"));
    let lines = [
        json!({"type":"session","version":3,"id":IMAGE_SESSION,
            "cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        json!({"type":"message","id":"user","parentId":null,"timestamp":"2026-10-21T00:00:01Z",
            "message":{"role":"user","timestamp":1000,
                "content":[{"type":"text","text":"  look   here "},image]}}),
    ]
    .map(|line| line.to_string());
    write_lines(&path, &lines);
    path
}

/// A client whose OMP storage is isolated under `root/agent`, whatever the test process's
/// own environment selects.
fn isolated_client(root: &Path) -> OmpClient {
    let mut client = OmpClient::new("unused".into()).with_image_directory(root.join("images"));
    client.environment =
        provider_core::ports::environment::AgentEnvironment::try_from(BTreeMap::from([
            ("HOME".to_owned(), root.to_string_lossy().into_owned()),
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                root.join("agent").to_string_lossy().into_owned(),
            ),
            ("PI_CODING_AGENT_SESSION_DIR".to_owned(), String::new()),
            ("OMP_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap();
    client
}

#[test]
fn unavailable_history_images_degrade_to_placeholders_without_affecting_live_output() {
    let root = tempfile::tempdir().unwrap();
    let missing = format!("blob:sha256:{}", "0".repeat(64));
    let path = image_session(
        root.path(),
        &json!({"type":"image","mimeType":"image/png","data":missing}),
    );

    let (history, stream) = read_journal(path)
        .project(&isolated_client(root.path()), &handle(IMAGE_SESSION, None))
        .unwrap();

    assert_eq!(history.entries.len(), 1);
    assert!(
        history.entries[0].item["text"]
            .as_str()
            .unwrap()
            .ends_with(provider_native::images::UNAVAILABLE_IMAGE)
    );
    assert_eq!(
        history.descriptor.first_prompt_preview.as_deref(),
        Some("look here")
    );
    assert!(
        stream
            .text(&json!([{"type":"image","data":"invalid"}]))
            .is_err()
    );
}

#[tokio::test]
async fn listing_summarizes_records_without_materializing_images() {
    let root = tempfile::tempdir().unwrap();
    image_session(
        root.path(),
        &json!({"type":"image","mimeType":"image/png","data":"aW1hZ2U="}),
    );

    let sessions = list(
        &isolated_client(root.path()),
        &ListOptions {
            cwd: Some("/tmp".to_owned()),
            scan_limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].provider_label, "Oh My Pi");
    assert_eq!(sessions[0].provider_handle_id, IMAGE_SESSION);
    assert_eq!(
        sessions[0].first_prompt_preview.as_deref(),
        Some("look here")
    );
    // Without a native title or a readable file name, the first prompt titles the session.
    assert_eq!(sessions[0].title.as_deref(), Some("look here"));
    assert_eq!(sessions[0].last_activity_at, "2026-10-21T00:00:01Z");
    assert!(!root.path().join("images").exists());
}

#[tokio::test]
async fn handles_restore_from_their_session_file_or_a_native_handle_path() {
    let root = tempfile::tempdir().unwrap();
    let path = image_session(
        root.path(),
        &json!({"type":"image","mimeType":"image/png","data":"aW1hZ2U="}),
    );
    let client = isolated_client(root.path());
    let recorded = BTreeMap::from([("sessionFile".to_owned(), json!(path))]);
    let mut native = handle(IMAGE_SESSION, None);
    native.native_handle = Some(json!(path));

    for handle in [handle(IMAGE_SESSION, Some(recorded)), native] {
        let history = inspect(&client, &handle, "/tmp").await.unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.descriptor.provider_handle_id, IMAGE_SESSION);
    }
    // Without a path, the session is found by its file name.
    assert!(
        inspect(&client, &handle(IMAGE_SESSION, None), "/tmp")
            .await
            .is_ok()
    );
    assert!(matches!(
        inspect(&client, &handle(IMAGE_SESSION, None), "/elsewhere").await,
        Err(AgentSessionError::Rejected)
    ));
}

/// A real OMP 18.18.8 `/skill:review x` expansion record, stamped with its message's time.
fn skill_record(parent: &str) -> Value {
    json!({"type":"custom_message","customType":"skill-prompt","display":true,
        "content":"[IMPORTANT: User invoked the \"review\" skill]\nUser: x",
        "details":{"name":"review","path":"/skills/review/SKILL.md","args":"x",
            "prompt":"/skill:review x","lineCount":1},
        "attribution":"user","id":"c03bdb88","parentId":parent,
        "timestamp":"1970-01-01T00:00:02.000Z"})
}

#[test]
fn pre_b4_handles_restore_client_ids_by_anchor_and_name_users_by_native_entry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.jsonl");
    let journal = journal(
        path.clone(),
        vec![record("84d68dc1", None, "hello"), skill_record("84d68dc1")],
    );
    // Written before D9: `inputIds` keyed by anchor keys, no native ids anywhere.
    let input_ids = json!({"native:omp:id:user:1000:0":"client-1",
        "native:omp:id:custom:2000:0":"client-2"});
    let handle = handle(
        "id",
        Some(BTreeMap::from([
            ("cwd".to_owned(), json!("/tmp")),
            ("sessionFile".to_owned(), json!(path)),
            ("inputIds".to_owned(), input_ids.clone()),
            ("empty".to_owned(), json!(false)),
            ("createdAt".to_owned(), json!("2026-10-21T00:00:00Z")),
        ])),
    );

    let (history, _) = journal
        .project(&OmpClient::new("missing".into()), &handle)
        .unwrap();

    assert_eq!(history.entries.len(), 2);
    assert_eq!(history.entries[0].key, "native:omp:id:user:1000:0");
    assert_eq!(
        history.entries[0].item,
        json!({"type":"user_message","messageId":"84d68dc1","text":"hello",
            "clientMessageId":"client-1"})
    );
    assert_eq!(history.entries[1].key, "native:omp:id:custom:2000:0");
    assert_eq!(
        history.entries[1].item,
        json!({"type":"user_message","messageId":"native:omp:id:custom:2000:0",
            "text":"/skill:review x","clientMessageId":"client-2"})
    );
    assert_eq!(history.resume_metadata["inputIds"], input_ids);
}

#[test]
fn live_user_messages_and_skill_prompts_render_as_history_does() {
    let root = tempfile::tempdir().unwrap();
    let user = record("84d68dc1", None, "hello");
    let skill = skill_record("84d68dc1");
    let journal = journal(
        root.path().join("history.jsonl"),
        vec![user.clone(), skill.clone()],
    );
    let mut live = Stream::new("id".into(), provider_native::images::ImageStore::default())
        .with_identity_lookups();
    live.pending_inputs.extend([
        ("prompt".to_owned(), Some("client-1".to_owned())),
        ("skill".to_owned(), Some("client-2".to_owned())),
    ]);
    // OMP 18.18.8 announces neither message with an entry id.
    live.message(&user["message"]).unwrap();
    live.resolve_identities(1, Some(&json!([{"entryId":"84d68dc1","text":"hello"}])));
    live.message(&record_message(&skill).unwrap()).unwrap();
    let live_entries = timeline(&mut live);
    let handle = handle(
        "id",
        Some(BTreeMap::from([(
            "inputIds".to_owned(),
            json!(live.input_ids),
        )])),
    );

    let (history, _) = journal
        .project(&OmpClient::new("missing".into()), &handle)
        .unwrap();

    assert_eq!(history.entries, live_entries);
}

#[test]
fn handles_with_more_than_4096_client_ids_restore_the_newest() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.jsonl");
    let journal = journal(path.clone(), vec![record("84d68dc1", None, "hello")]);
    // Written before the map was bounded: one id per message, oldest first.
    let mut input_ids: serde_json::Map<String, Value> = (0..5000)
        .map(|index| {
            (
                format!("native:omp:id:user:{}:0", 1_000_000 + index),
                json!(format!("old-{index}")),
            )
        })
        .collect();
    input_ids.insert("native:omp:id:user:1000:0".to_owned(), json!("client-1"));
    input_ids.insert("native:omp:id:user:9999999:0".to_owned(), json!("newest"));
    let handle = handle(
        "id",
        Some(BTreeMap::from([(
            "inputIds".to_owned(),
            Value::Object(input_ids),
        )])),
    );

    let (history, _) = journal
        .project(&OmpClient::new("missing".into()), &handle)
        .unwrap();

    let restored = history.resume_metadata["inputIds"].as_object().unwrap();
    assert_eq!(restored.len(), 4096);
    assert_eq!(restored["native:omp:id:user:9999999:0"], "newest");
    assert!(!restored.contains_key("native:omp:id:user:1000:0"));
    assert!(!restored.contains_key("native:omp:id:user:1000000:0"));
    assert!(restored.contains_key("native:omp:id:user:1004999:0"));
    // The oldest message lost its client identity rather than failing the read.
    assert_eq!(history.entries[0].item.get("clientMessageId"), None);
}
