use std::collections::BTreeMap;

use provider_core::ports::environment::AgentEnvironment;

use super::*;

const PARENT: &str = "01a1ca98-b59b-724c-916c-215b8f600344";

fn client(root: &Path) -> OmpClient {
    let mut client = OmpClient::new("unused".into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
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

fn write(path: &Path, lines: &[Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = lines
        .iter()
        .fold(String::new(), |text, line| text + &line.to_string() + "\n");
    std::fs::write(path, text).unwrap();
}

fn message(id: &str, parent: Option<&str>, message: &Value) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"timestamp":"2026-10-21T00:00:01Z",
        "message":message})
}

fn task_call(call: &str, parent: &str) -> Value {
    message(
        call,
        Some(parent),
        &json!({"role":"assistant","timestamp":1000,"stopReason":"toolUse","content":[
            {"type":"toolCall","id":call,"name":"task","arguments":{"i":"delegate","agent":"explore",
                "tasks":[{"task":"# Target\nReview","name":"Prober"}]}}]}),
    )
}

/// An OMP 18.18.8 child transcript: title slot, header naming the parent, `session_init`.
fn child(path: &Path, parent_file: &Path, cwd: &str, done: bool) {
    let mut lines = vec![
        json!({"type":"title","v":1,"title":""}),
        json!({"type":"session","version":3,"id":"01a1ca98-b5c0-7000-8000-000000000000",
            "timestamp":"2026-10-21T00:00:02Z","cwd":cwd,"parentSession":parent_file}),
        json!({"type":"model_change","id":"m","parentId":null,"timestamp":"2026-10-21T00:00:02Z",
            "model":"ait-local-fixture/fixture"}),
        json!({"type":"session_init","id":"init","parentId":"m",
            "timestamp":"2026-10-21T00:00:02Z","systemPrompt":["system"]}),
        message(
            "u",
            Some("init"),
            &json!({"role":"user","timestamp":2000,"attribution":"agent",
                "content":[{"type":"text","text":"Complete assignment"}]}),
        ),
    ];
    if done {
        lines.push(message(
            "a",
            Some("u"),
            &json!({"role":"assistant","timestamp":2100,"stopReason":"stop",
                "provider":"ait-local-fixture","model":"fixture",
                "content":[{"type":"text","text":"Child done"}]}),
        ));
    }
    write(path, &lines);
}

/// A parent session in `root`'s default layout whose background task started `Prober`.
fn parent(root: &Path, cwd: &str) -> PathBuf {
    let file = root
        .join("agent/sessions/--work--")
        .join(format!("2026-10-21T00-00-00-000Z_{PARENT}.jsonl"));
    write(
        &file,
        &[
            json!({"type":"title","v":1,"title":""}),
            json!({"type":"session","version":3,"id":PARENT,"cwd":cwd,
                "timestamp":"2026-10-21T00:00:00Z"}),
            message(
                "u",
                None,
                &json!({"role":"user","timestamp":900,"content":"delegate"}),
            ),
            task_call("call-0", "u"),
            // OMP 18.18.8 runs a task in the background and names its job.
            message(
                "r",
                Some("call-0"),
                &json!({"role":"toolResult","timestamp":1100,"toolCallId":"call-0",
                    "toolName":"task","content":[{"type":"text","text":"Spawned agent `Prober`."}],
                    "details":{"results":[],"progress":[{"index":0,"id":"Prober","agent":"task",
                        "status":"pending","assignment":"# Target\nReview"}],
                        "async":{"state":"running","jobId":"Prober","type":"task"}}}),
            ),
            json!({"type":"custom_message","customType":"async-result","id":"done",
                "parentId":"r","timestamp":"2026-10-21T00:00:03Z","display":true,
                "content":"<system-notice>\nBackground job Prober has completed.\n</system-notice>",
                "details":{"jobs":[{"jobId":"Prober","type":"task","status":"failed",
                    "agentId":"Prober"}]}}),
        ],
    );
    child(&transcript(&file, "Prober"), &file, cwd, true);
    file
}

#[test]
fn task_results_link_finished_and_background_children() {
    let finished = message(
        "r",
        Some("c"),
        &json!({"role":"toolResult","toolCallId":"c","toolName":"task","isError":false,
            "details":{"results":[
                {"id":"ok","agent":"reviewer","exitCode":0,"description":"Check"},
                {"id":"bad","exitCode":2},
                {"id":"stop","aborted":true},
                {"id":"../escape"}]}}),
    );
    let entries = [task_call("c", "u"), finished];

    let links = links(&entries);

    assert_eq!(links.len(), 3);
    assert_eq!(
        links["ok"],
        Link {
            call: "c".to_owned(),
            agent: Some("reviewer".to_owned()),
            description: Some("Check".to_owned()),
            status: Some("completed"),
        }
    );
    assert_eq!(links["bad"].agent.as_deref(), Some("explore"));
    assert_eq!(links["bad"].status, Some("failed"));
    assert_eq!(links["stop"].status, Some("canceled"));
    // A result whose call is not on the branch links nothing, as in Paseo.
    assert!(super::links(&entries[1..]).is_empty());
}

#[tokio::test]
async fn children_are_discovered_from_parent_transcripts_and_inspected_by_their_handles() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let cwd = cwd.to_str().unwrap();
    let parent_file = parent(root.path(), cwd);
    let client = client(root.path());

    let children = list(&client, cwd).unwrap();

    assert_eq!(children.len(), 1);
    let child = &children[0];
    let id = format!("{PARENT}/Prober");
    assert_eq!(child.id, id);
    assert_eq!(child.parent_id, PARENT);
    assert_eq!(
        child.descriptor["title"],
        "task · fixture (ait-local-fixture)"
    );
    assert_eq!(child.descriptor["description"], "# Target\nReview");
    assert_eq!(child.descriptor["status"], "failed");
    assert_eq!(child.descriptor["toolCallId"], "call-0");
    assert_eq!(child.descriptor["createdAt"], "2026-10-21T00:00:02Z");
    let handle = child.persistence.clone().unwrap();
    assert_eq!(handle.session_id, id);

    let history = super::super::inspect(&client, &handle, cwd).await.unwrap();

    assert_eq!(history.parent_id.as_deref(), Some(PARENT));
    assert_eq!(history.descriptor.provider_handle_id, id);
    assert_eq!(history.descriptor.title.as_deref(), Some("Prober"));
    assert_eq!(history.entries.len(), 2);
    assert_eq!(history.entries[1].item["text"], "Child done");
    assert!(
        history.entries[1]
            .key
            .starts_with(&format!("native:omp:{id}:"))
    );
    assert!(!history.active);
    assert_eq!(
        history.resume_metadata[LOCATOR]["parentFile"],
        json!(parent_file)
    );
    assert!(list(&client, "/elsewhere").unwrap().is_empty());
}

#[test]
fn nested_children_name_their_parent_child_and_unfinished_ones_are_running() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_str().unwrap();
    let parent_file = parent(root.path(), cwd);
    let child_file = transcript(&parent_file, "Prober");
    // The child started its own background task, `Deep`, which has not finished.
    let mut lines: Vec<Value> = std::fs::read_to_string(&child_file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    lines.push(task_call("nested", "a"));
    lines.push(message(
        "nested-result",
        Some("nested"),
        &json!({"role":"toolResult","toolCallId":"nested","toolName":"task",
            "details":{"results":[],"async":{"jobId":"Deep"}}}),
    ));
    write(&child_file, &lines);
    child(&transcript(&child_file, "Deep"), &child_file, cwd, false);

    let children = list(&client(root.path()), cwd).unwrap();

    assert_eq!(children.len(), 2);
    let deep = &children[1];
    assert_eq!(deep.id, format!("{PARENT}/Prober/Deep"));
    assert_eq!(deep.parent_id, format!("{PARENT}/Prober"));
    assert_eq!(deep.descriptor["status"], "running");
    assert_eq!(
        deep.descriptor["title"],
        "explore · fixture (ait-local-fixture)"
    );
}

#[test]
fn child_handles_are_validated_before_any_transcript_is_read() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_str().unwrap();
    let parent_file = parent(root.path(), cwd);
    let client = client(root.path());
    let valid = handle(&parent_file, PARENT, "Prober");
    assert!(read(&client, &valid, cwd).is_ok());
    assert!(matches!(
        read(&client, &valid, "/elsewhere"),
        Err(AgentSessionError::Rejected)
    ));

    let mut renamed = valid.clone();
    renamed.session_id = format!("{PARENT}/Other");
    let escaping = handle(&parent_file, PARENT, "..");
    let relative = handle(Path::new("parent.jsonl"), PARENT, "Prober");
    for invalid in [renamed, escaping, relative] {
        assert!(matches!(
            read(&client, &invalid, cwd),
            Err(AgentSessionError::Rejected)
        ));
    }
    assert!(matches!(
        read(&client, &handle(&parent_file, PARENT, "Missing"), cwd),
        Err(AgentSessionError::Unavailable)
    ));

    // A transcript OMP recorded for another parent session is not this parent's child.
    let other = root.path().join("other.jsonl");
    std::fs::write(&other, "").unwrap();
    child(&transcript(&parent_file, "Prober"), &other, cwd, true);
    assert!(matches!(
        read(&client, &valid, cwd),
        Err(AgentSessionError::Rejected)
    ));
}

#[test]
fn job_ids_must_name_a_file_in_the_parent_directory() {
    for valid in ["Prober", "0-Explore", "a.b_c"] {
        assert!(valid_job(valid), "{valid}");
    }
    for invalid in ["", ".", "..", "a/b", "a\\b", "名", &"a".repeat(129)] {
        assert!(!valid_job(invalid), "{invalid}");
    }
}
