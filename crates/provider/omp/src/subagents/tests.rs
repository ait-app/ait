use super::*;

fn root(file: Option<&Path>) -> Parent<'_> {
    Parent {
        id: "root",
        file,
        cwd: "/tmp",
    }
}

#[test]
fn forwards_only_children_announced_by_the_owned_native_process() {
    let mut children = Children::default();
    let images = ImageStore::default();
    let message = json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","timestamp":1000,"content":[{"type":"text","text":"Hello"}]}}}});
    assert_eq!(
        children.consume(&message, root(None), &images).unwrap(),
        Vec::new()
    );
    let lifecycle = json!({"type":"subagent_lifecycle","payload":{"id":"child","agent":"Worker","status":"started","parentToolCallId":"call"}});
    assert_eq!(
        children
            .consume(&lifecycle, root(None), &images)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(children.list()[0].parent_id, "root");
    assert_eq!(children.list()[0].id, "root/child");
    let events = children.consume(&message, root(None), &images).unwrap();
    assert!(
        matches!(&events[0],AgentTurnEvent::Subagent(SubagentEvent::Timeline{id,entry}) if id=="root/child" && entry.item["text"]=="Hello")
    );
    let invalid =
        json!({"type":"subagent_lifecycle","payload":{"id":"../child","status":"started"}});
    assert!(children.consume(&invalid, root(None), &images).is_err());
}

#[test]
fn descriptors_follow_paseo_titles_descriptions_and_locate_the_child_transcript() {
    let mut children = Children::default();
    let images = ImageStore::default();
    let file = Path::new("/sessions/2026_root.jsonl");
    // OMP 18.18.8 lifecycle and progress payloads (B6/B7 probes).
    let lifecycle = json!({"type":"subagent_lifecycle","payload":{"id":"Prober","agent":"task",
        "parentToolCallId":"call-0","detached":true,"status":"started","index":0,
        "sessionFile":"/sessions/2026_root/Prober.jsonl"}});
    children
        .consume(&lifecycle, root(Some(file)), &images)
        .unwrap();
    let child = &children.list()[0];
    assert_eq!(child.descriptor["title"], "task");
    assert_eq!(child.descriptor["description"], Value::Null);
    assert_eq!(
        child.persistence,
        Some(children::handle(file, "root", "Prober"))
    );

    let progress = json!({"type":"subagent_progress","payload":{"index":0,"agent":"task",
        "task":"Complete assignment thoroughly:\n\n# Target\nReview","parentToolCallId":"call-0",
        "assignment":"# Target\nReview","progress":{"index":0,"id":"Prober","status":"running",
            "resolvedModel":"ait-local-fixture/fixture"}}});
    children
        .consume(&progress, root(Some(file)), &images)
        .unwrap();
    let completed = json!({"type":"subagent_lifecycle","payload":{"id":"Prober","agent":"task",
        "status":"completed","index":0,"description":"Reviewed"}});
    children
        .consume(&completed, root(Some(file)), &images)
        .unwrap();

    let child = &children.list()[0];
    assert_eq!(
        child.descriptor["title"],
        "task · fixture (ait-local-fixture)"
    );
    assert_eq!(child.descriptor["description"], "Reviewed");
    assert_eq!(child.descriptor["toolCallId"], "call-0");
    assert_eq!(child.descriptor["status"], "completed");
    assert!(child.descriptor.get("subtitle").is_none());
}

#[test]
fn titles_name_the_model_and_its_provider() {
    assert_eq!(title(None, None), "OMP subagent");
    assert_eq!(title(Some("  "), Some("model")), "OMP subagent · model");
    assert_eq!(
        title(Some("explore"), Some("anthropic/opus")),
        "explore · opus (anthropic)"
    );
    assert_eq!(title(Some("explore"), Some("/opus")), "explore · /opus");
    assert_eq!(title(Some("explore"), Some(" ")), "explore");
}

#[test]
fn unknown_child_status_and_unprojectable_child_events_do_not_fail_the_parent() {
    let mut children = Children::default();
    let images = ImageStore::default();
    let started = json!({"type":"subagent_lifecycle","payload":{"id":"child","status":"running"}});
    children.consume(&started, root(None), &images).unwrap();
    let unknown = json!({"type":"subagent_lifecycle","payload":{"id":"child","status":"paused"}});

    let ignored = children.consume(&unknown, root(None), &images).unwrap();
    let invalid = json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","content":[]}}}});
    let dropped = children.consume(&invalid, root(None), &images).unwrap();

    assert!(ignored.is_empty());
    assert!(dropped.is_empty());
    assert_eq!(children.list()[0].descriptor["status"], "running");
    let message = json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","timestamp":1000,"content":[{"type":"text","text":"Hello"}]}}}});
    assert_eq!(
        children
            .consume(&message, root(None), &images)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn child_images_use_the_native_blob_store() {
    use sha2::{Digest, Sha256};
    let directory = tempfile::tempdir().unwrap();
    let hash = format!("{:x}", Sha256::digest(b"child-image"));
    std::fs::write(directory.path().join(&hash), b"child-image").unwrap();
    let mut children = Children::default().with_blobs(directory.path().to_path_buf());
    let images = ImageStore::new(directory.path().join("images"));
    children
        .consume(
            &json!({"type":"subagent_lifecycle","payload":{"id":"child","status":"started"}}),
            root(None),
            &images,
        )
        .unwrap();
    let events = children.consume(&json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","timestamp":1000,"content":[{"type":"image","mimeType":"image/png","data":format!("blob:sha256:{hash}")}]}}}}), root(None), &images).unwrap();
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) if entry.item["text"].as_str().unwrap().contains("![Image]"))));
}

#[test]
fn terminalization_cancels_only_running_children_once() {
    let mut children = Children::default();
    let images = ImageStore::default();
    for (id, status) in [("running", "started"), ("done", "completed")] {
        let lifecycle = json!({"type":"subagent_lifecycle","payload":{"id":id,"agent":"Task",
            "status":status,"parentToolCallId":"call"}});
        children.consume(&lifecycle, root(None), &images).unwrap();
    }

    let events = children.terminalize();

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], AgentTurnEvent::Subagent(SubagentEvent::Upsert(child))
            if child.id == "root/running" && child.descriptor["status"] == "canceled")
    );
    assert!(children.terminalize().is_empty());
    let statuses: Vec<_> = children
        .list()
        .into_iter()
        .map(|child| child.descriptor["status"].clone())
        .collect();
    assert_eq!(statuses, [json!("completed"), json!("canceled")]);
}

#[test]
fn past_the_child_limit_finished_children_make_room_oldest_first() {
    let mut children = Children::default();
    let images = ImageStore::default();
    let lifecycle = |job: &str, status: &str| json!({"type":"subagent_lifecycle","payload":{"id":job,"status":status}});
    for index in 0..MAX_CHILDREN {
        let status = if index < 2 { "completed" } else { "started" };
        children
            .consume(
                &lifecycle(&format!("job-{index}"), status),
                root(None),
                &images,
            )
            .unwrap();
    }
    // The newest finished child is not the oldest one.
    children
        .consume(&lifecycle("job-5", "failed"), root(None), &images)
        .unwrap();

    for job in ["late-0", "late-1", "late-2"] {
        let events = children
            .consume(&lifecycle(job, "started"), root(None), &images)
            .unwrap();
        assert_eq!(events.len(), 1, "{job}");
    }

    let ids: Vec<_> = children.list().into_iter().map(|child| child.id).collect();
    assert_eq!(ids.len(), MAX_CHILDREN);
    for gone in ["root/job-0", "root/job-1", "root/job-5"] {
        assert!(!ids.iter().any(|id| id == gone), "{gone}");
    }
    assert!(ids.iter().any(|id| id == "root/late-2"));

    // With every tracked child running, a new child's frame is dropped, not fatal.
    let events = children
        .consume(&lifecycle("late-3", "started"), root(None), &images)
        .unwrap();
    assert!(events.is_empty());
    assert_eq!(children.list().len(), MAX_CHILDREN);
    assert!(
        !children
            .list()
            .iter()
            .any(|child| child.id == "root/late-3")
    );
}
