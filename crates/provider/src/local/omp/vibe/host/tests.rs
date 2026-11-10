use super::*;

fn host() -> Host {
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: "/project".to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    Host::new(
        &OmpClient::new("unused".into()),
        &spec,
        "parent",
        "/parent.jsonl",
        None,
    )
}

#[tokio::test]
async fn disabled_and_foreign_controls_are_rejected_without_starting_a_process() {
    let host = host();
    assert!(
        operate(host.shared.clone(), &BTreeMap::new(), "spawn", &json!({}))
            .await
            .is_err()
    );
    host.shared.enabled.store(true, Ordering::Release);
    for operation in ["send", "kill"] {
        assert!(
            operate(
                host.shared.clone(),
                &BTreeMap::new(),
                operation,
                &json!({"session":"foreign","message":"hello"})
            )
            .await
            .is_err()
        );
    }
    for fields in [
        json!({"timeout":0}),
        json!({"timeout":601}),
        json!({"sessions":["foreign"]}),
    ] {
        assert!(wait(host.shared.clone(), &fields).await.is_err());
    }
    assert_eq!(host.snapshot(), json!({"enabled":true,"workers":[]}));
}

#[test]
fn a_dropped_launch_releases_its_single_writer_reservation() {
    let host = host();
    host.shared
        .registry
        .lock()
        .unwrap()
        .starting
        .insert("worker".to_owned(), None);
    let reservation = Reservation {
        shared: host.shared.clone(),
        key: "worker".to_owned(),
    };
    assert!(host.running());
    drop(reservation);
    assert!(!host.running());
}

#[test]
fn extension_and_host_tools_use_only_the_public_external_api() {
    let source = std::fs::read_to_string(Host::extension().unwrap().path()).unwrap();
    assert!(source.contains("before_agent_start"));
    assert!(source.contains("pi.setActiveTools"));
    assert!(!source.contains("import "));
    let definitions = tools::definitions();
    assert_eq!(definitions.len(), 5);
    assert!(
        definitions
            .iter()
            .all(|tool| tool["loadMode"] == "essential")
    );
    let commands = visible_commands(
        &[
            json!({"name":COMMAND,"source":"extension"}),
            json!({"name":"ait-vibe-result-v1","source":"extension"}),
            json!({"name":"ait-btw-branch-v1","source":"extension"}),
        ],
        true,
    );
    assert!(commands.iter().any(|command| command["name"] == "vibe"));
    assert!(
        commands
            .iter()
            .all(|command| command["name"] != "ait-btw-branch-v1")
    );
    assert!(
        commands
            .iter()
            .all(|command| !command["name"].as_str().unwrap().starts_with("ait-vibe-"))
    );
}

#[test]
fn restart_does_not_enable_vibe_or_accept_another_parents_worker() {
    let mut host = host();
    let record = Record {
        child: NativeSubagent {
            id: "worker".to_owned(),
            parent_id: "parent".to_owned(),
            cwd: "/project".to_owned(),
            persistence: Some(AgentPersistenceHandle {
                provider: "omp".to_owned(),
                session_id: "worker".to_owned(),
                native_handle: None,
                metadata: None,
            }),
            descriptor: json!({"status":"running"}),
        },
        cli: "fast".to_owned(),
        config: StoredAgentConfig::default(),
        answer: "result".to_owned(),
    };
    host.shared
        .registry
        .lock()
        .unwrap()
        .records
        .insert("worker".to_owned(), record);
    let stored = host.persisted();
    let restored = Host::new(
        &host.shared.client,
        &host.shared.spec,
        "parent",
        "/parent.jsonl",
        Some(&stored),
    );
    assert!(!restored.enabled());
    assert_eq!(restored.children().len(), 1);
    assert_eq!(restored.children()[0].descriptor["status"], "canceled");
    let foreign = Host::new(
        &host.shared.client,
        &host.shared.spec,
        "other",
        "/other.jsonl",
        Some(&stored),
    );
    assert!(foreign.children().is_empty());
    host.stop();
    assert!(host.pending().is_empty());
}

#[test]
fn failed_handshake_and_oversized_results_do_not_claim_success() {
    let host = host();
    let command = json!({"name":COMMAND,"source":"extension"});
    assert!(
        Host::discover(
            &host.shared.client,
            &host.shared.spec,
            &json!({}),
            &[],
            None
        )
        .unwrap()
        .is_none()
    );
    assert!(
        Host::discover(
            &host.shared.client,
            &host.shared.spec,
            &json!({}),
            &[command],
            None
        )
        .is_err()
    );
    let ordinary = tools::result(&json!({"session":"worker"}), false);
    assert_eq!(ordinary["isError"], false);
    let huge = tools::result(&json!({"answer":"\0".repeat(1024 * 1024)}), false);
    assert_eq!(huge["isError"], true);
    assert!(huge.to_string().len() < 1024);
}

#[tokio::test]
async fn cancellation_aborts_a_worker_even_before_its_start_receipt_arrives() {
    let mut host = host();
    let (mut sender, receiver) = oneshot::channel::<()>();
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        let _ = receiver.await;
    }));
    host.shared
        .registry
        .lock()
        .unwrap()
        .starting
        .insert("starting".to_owned(), Some(task));
    assert!(host.running());
    host.stop();
    tokio::time::timeout(Duration::from_secs(1), sender.closed())
        .await
        .unwrap();
    assert!(!host.running());
    assert_eq!(host.shared.generation.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn exit_during_a_director_turn_preserves_cancellation_frames_for_the_owner() {
    let (_root, client, spec) = crate::local::omp::tests::fixture();
    let mut transport = Transport::spawn(&client, &spec.cwd, &[], None).unwrap();
    let state = transport.request("get_state", json!({})).await.unwrap();
    let mut host = Host::new(
        &client,
        &spec,
        state["sessionId"].as_str().unwrap(),
        state["sessionFile"].as_str().unwrap(),
        None,
    );
    host.shared.enabled.store(true, Ordering::Release);
    let prompt = transport
        .begin("prompt", json!({"message":"wait"}))
        .await
        .unwrap();
    host.toggle(&mut transport, false).await.unwrap();
    assert!(!host.enabled());
    let mut frames = Vec::new();
    while let Some(frame) = transport.poll().unwrap() {
        frames.push(frame);
    }
    assert!(
        frames
            .iter()
            .any(|frame| frame["type"] == "response" && frame["id"] == prompt)
    );
    assert!(
        frames.iter().any(|frame| frame["type"] == "prompt_result"
            && frame["id"] == prompt
            && frame["status"] == "aborted"),
        "exit must retain the parent's native aborted outcome"
    );
    transport.close().await.unwrap();
}
