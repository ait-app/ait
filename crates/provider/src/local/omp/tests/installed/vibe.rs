use super::*;

pub(super) fn completion(request: &Value) -> Option<Value> {
    let director = request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == "vibe_spawn")
    });
    if !director
        && request["messages"]
            .to_string()
            .contains("AIT_VIBE_THINKING_FIXTURE")
    {
        let text = request["messages"].to_string();
        let level = text
            .split("AIT_NATIVE_THINKING=")
            .nth(1)
            .expect("native thinking hook ran");
        let level: String = level
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        return Some(
            json!({"role":"assistant","content":format!("Native worker thinking: {level}")}),
        );
    }
    if !request["messages"]
        .to_string()
        .contains("AIT_VIBE_DIRECTOR_FIXTURE")
    {
        return director
            .then(|| json!({"role":"assistant","content":"Vibe worker report received"}));
    }
    let tools: Vec<_> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(
        tools.contains(&"vibe_spawn"),
        "missing Vibe tools: {tools:?}"
    );
    assert!(
        !tools.contains(&"write") && !tools.contains(&"bash"),
        "director has worker tools: {tools:?}"
    );
    let messages = request["messages"].as_array().unwrap();
    let spawned = messages
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "vibe-spawn");
    let waited = messages
        .iter()
        .any(|message| message["role"] == "tool" && message["tool_call_id"] == "vibe-wait");
    let (name, id, arguments) = if let Some(spawned) = spawned {
        if waited {
            return Some(json!({"role":"assistant","content":"VIBE_DIRECTOR_DONE"}));
        }
        let result: Value = serde_json::from_str(spawned["content"].as_str().unwrap()).unwrap();
        let worker = result["session"]
            .as_str()
            .expect("spawn must return a native worker UUID");
        (
            "vibe_wait",
            "vibe-wait",
            json!({"sessions":[worker],"timeout":60}),
        )
    } else {
        (
            "vibe_spawn",
            "vibe-spawn",
            json!({"cli":"fast","name":"writer","prompt":"OMP_WRITE_APPROVAL_FIXTURE"}),
        )
    };
    Some(
        json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]}),
    )
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; real RPC workers and loopback-only model inference"]
async fn installed_external_omp_vibe_director_spawns_waits_and_worker_writes_a_file() {
    let mut fixture = installed_with_plugins().await;
    fixture.spec.config.mode_id = Some("write".to_owned());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert!(
        session
            .commands()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "vibe")
    );
    assert!(
        !session
            .handles_out_of_band("/vibe AIT_VIBE_DIRECTOR_FIXTURE")
            .unwrap_or(false)
    );
    session
        .start_turn("/vibe AIT_VIBE_DIRECTOR_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();
    let events = tokio::time::timeout(Duration::from_secs(120), async {
        let mut events = Vec::new();
        loop {
            let Some(event) = session.poll_turn().unwrap() else { tokio::time::sleep(Duration::from_millis(10)).await; continue; };
            assert!(!matches!(event, AgentTurnEvent::Failed), "{events:?}: {:?}", session.failure_message());
            if let AgentTurnEvent::PermissionRequested(request) = &event {
                approve(session.as_mut(), request).await;
            }
            let complete = matches!(&event, AgentTurnEvent::Completed(Some(answer)) if answer.contains("VIBE_DIRECTOR_DONE"));
            events.push(event);
            if complete { break events; }
        }
    }).await.expect("Vibe did not complete");
    assert_eq!(
        std::fs::read_to_string(
            std::path::Path::new(&fixture.spec.cwd).join("approval-probe.html")
        )
        .unwrap(),
        "OMP_WRITE_OK"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Subagent(_))),
        "worker was not observed: {events:?}"
    );
    let workers = session.subagents();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].descriptor["status"], "completed");
    let parent = session.persistence().unwrap();
    let worker = workers[0].persistence.as_ref().unwrap();
    let path = worker.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let journal = std::fs::read_to_string(path).unwrap();
    let header: Value = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| entry["type"] == "session")
        .unwrap();
    assert_eq!(
        header["parentSession"],
        parent.metadata.as_ref().unwrap()["sessionFile"],
        "header={header}, worker={worker:?}"
    );
    session
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&parent, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(restored.ui_state().unwrap()["vibe"]["enabled"], false);
    assert_eq!(restored.subagents().len(), 1);
    restored.close().await.unwrap();
}

async fn approve(session: &mut dyn AgentSession, request: &Value) {
    let mut answers = serde_json::Map::new();
    for question in request["input"]["questions"].as_array().unwrap() {
        assert!(
            question["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["label"] == "Approve"),
            "unexpected question: {request}"
        );
        let key = question["answerKey"]
            .as_str()
            .or_else(|| question["header"].as_str())
            .unwrap();
        answers.insert(key.to_owned(), json!("Approve"));
    }
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":answers}}),
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; verifies native worker approval, follow-up and resume"]
async fn installed_external_omp_vibe_worker_approval_followup_resume_and_foreign_parent_rejection()
{
    let mut fixture = installed_with_plugins().await;
    fixture.spec.config.mode_id = Some("ask".to_owned());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    let receipt = control(session.as_mut(), json!({"operation":"spawn","cli":"good","name":"approved writer","prompt":"OMP_WRITE_APPROVAL_FIXTURE"})).await;
    let id = receipt["session"].as_str().unwrap().to_owned();
    let question = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Some(AgentTurnEvent::PermissionRequested(request)) = session.poll_turn().unwrap()
            {
                break request;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        question["id"]
            .as_str()
            .unwrap()
            .starts_with(&format!("ait-vibe:{id}:"))
    );
    assert!(
        !std::path::Path::new(&fixture.spec.cwd)
            .join("approval-probe.html")
            .exists()
    );
    approve(session.as_mut(), &question).await;
    worker_done(session.as_mut(), &id).await;
    assert!(session.pending_permissions().is_empty());
    assert_eq!(
        std::fs::read_to_string(
            std::path::Path::new(&fixture.spec.cwd).join("approval-probe.html")
        )
        .unwrap(),
        "OMP_WRITE_OK"
    );
    control(
        session.as_mut(),
        json!({"operation":"send","sessionId":id,"message":"Same-worker follow-up"}),
    )
    .await;
    worker_done(session.as_mut(), &id).await;
    let parent = session.persistence().unwrap();
    let worker = session
        .subagents()
        .into_iter()
        .find(|worker| worker.id == id)
        .unwrap();
    let handle = worker.persistence.as_ref().unwrap();
    assert_worker_message(&fixture, handle, "Same-worker follow-up").await;
    reject_foreign_parent(&fixture, handle).await;
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&parent, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    restored
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    control(
        restored.as_mut(),
        json!({"operation":"send","sessionId":id,"message":"Restored-worker follow-up"}),
    )
    .await;
    worker_done(restored.as_mut(), &id).await;
    let worker = restored
        .subagents()
        .into_iter()
        .find(|worker| worker.id == id)
        .unwrap();
    assert_worker_message(
        &fixture,
        worker.persistence.as_ref().unwrap(),
        "Restored-worker follow-up",
    )
    .await;
    control(
        restored.as_mut(),
        json!({"operation":"kill","sessionId":id}),
    )
    .await;
    restored.close().await.unwrap();
}

async fn control(session: &mut dyn AgentSession, arguments: Value) -> Value {
    let future = session
        .begin_action("vibe_control", &arguments)
        .await
        .unwrap();
    let receipt = future.await.unwrap();
    assert!(receipt["error"].is_null(), "{receipt}");
    serde_json::from_str(receipt["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; 33 sequential native workers with loopback model responses"]
async fn installed_external_omp_vibe_can_spawn_after_32_completed_workers() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    let mut first_journal = None;
    for index in 0..33 {
        let result = control(session.as_mut(), json!({"operation":"spawn", "cli":"fast", "prompt":"Reply with one word", "name":format!("worker-{index}")})).await;
        let id = result["session"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                while session.poll_turn().unwrap().is_some() {}
                if let Some(worker) = session
                    .subagents()
                    .into_iter()
                    .find(|worker| worker.id == id && worker.descriptor["status"] == "completed")
                {
                    if index == 0 {
                        first_journal = Some(
                            worker.persistence.unwrap().metadata.unwrap()["sessionFile"]
                                .as_str()
                                .unwrap()
                                .to_owned(),
                        );
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(session.subagents().len() <= 32);
    }
    assert!(std::path::Path::new(&first_journal.unwrap()).is_file());
    session.close().await.unwrap();
}

async fn worker_done(session: &mut dyn AgentSession, id: &str) {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut reported = false;
        loop {
            while let Some(event) = session.poll_turn().unwrap() {
                reported |= matches!(event, AgentTurnEvent::Completed(_));
                assert!(
                    !matches!(
                        event,
                        AgentTurnEvent::Failed | AgentTurnEvent::PermissionRequested(_)
                    ),
                    "unexpected worker event: {event:?}"
                );
            }
            if reported
                && session
                    .subagents()
                    .iter()
                    .any(|worker| worker.id == id && worker.descriptor["status"] == "completed")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker did not complete");
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; aborts a director blocked on native worker approval"]
async fn installed_external_omp_vibe_cancel_stops_worker_and_clears_pending_approval() {
    let mut fixture = installed_with_plugins().await;
    fixture.spec.config.mode_id = Some("ask".to_owned());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("/vibe AIT_VIBE_DIRECTOR_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();
    let worker = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                if let AgentTurnEvent::PermissionRequested(request) = &event {
                    if request["id"].as_str().unwrap().starts_with("ait-vibe:") {
                        break session.subagents()[0].id.clone();
                    }
                    approve(session.as_mut(), request).await;
                }
                assert!(
                    !matches!(event, AgentTurnEvent::Failed | AgentTurnEvent::Completed(_)),
                    "director ended before worker approval: {event:?}"
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !std::path::Path::new(&fixture.spec.cwd)
            .join("approval-probe.html")
            .exists()
    );
    control(
        session.as_mut(),
        json!({"operation":"send","sessionId":worker,"message":"Steer while waiting for approval"}),
    )
    .await;
    session.cancel_turn(&turn).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Cancelled)),
        "{events:?}"
    );
    assert!(session.pending_permissions().is_empty());
    assert!(
        session
            .subagents()
            .iter()
            .all(|worker| worker.descriptor["status"] == "canceled")
    );
    assert!(
        !std::path::Path::new(&fixture.spec.cwd)
            .join("approval-probe.html")
            .exists()
    );
    session
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    session.close().await.unwrap();
}

async fn reject_foreign_parent(fixture: &Installed, handle: &AgentPersistenceHandle) {
    assert!(
        session::open_worker(
            &fixture.client,
            &fixture.spec,
            "/foreign.jsonl",
            Some(handle)
        )
        .await
        .is_err()
    );
}

async fn assert_worker_message(
    fixture: &Installed,
    handle: &AgentPersistenceHandle,
    message: &str,
) {
    let history = fixture
        .client
        .history(handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert!(history.iter().any(|entry| entry.item["text"] == message));
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; director high thinking and a non-reasoning fast worker"]
async fn installed_external_omp_vibe_fast_model_does_not_inherit_incompatible_director_thinking() {
    let mut fixture = installed_with_plugins().await;
    let path = std::path::Path::new(&fixture.spec.cwd).join("agent/models.yml");
    let mut models: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let entries = models["providers"]["ait-local-fixture"]["models"]
        .as_array_mut()
        .unwrap();
    let mut fast = entries[0].clone();
    fast["id"] = json!("fast-worker");
    entries[0]["reasoning"] = json!(true);
    entries[0]["thinking"] = json!({"mode":"effort","efforts":["low","high"]});
    entries.push(fast);
    std::fs::write(&path, models.to_string()).unwrap();
    fixture.spec.config.mode_id = Some("full".to_owned());
    fixture.spec.config.thinking_option_id = Some("high".to_owned());
    fixture.spec.config.feature_values = Some(BTreeMap::from([(
        "modelRole.smol".to_owned(),
        json!("ait-local-fixture/fast-worker"),
    )]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .out_of_band(&AgentPrompt::text("/vibe"))
        .await
        .unwrap();
    let receipt = control(
        session.as_mut(),
        json!({"operation":"spawn","cli":"fast","prompt":"Use the local fixture tool"}),
    )
    .await;
    let id = receipt["session"].as_str().unwrap();
    worker_done(session.as_mut(), id).await;
    let worker = session
        .subagents()
        .into_iter()
        .find(|worker| worker.id == id)
        .unwrap();
    assert_eq!(
        worker.descriptor["model"],
        json!(["ait-local-fixture", "fast-worker"]).to_string()
    );
    assert_eq!(worker.descriptor["status"], "completed");
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; checks native worker thinking with a named parent preset"]
async fn installed_external_omp_vibe_named_parent_preset_does_not_override_worker_native_thinking()
{
    let mut fixture = installed_with_plugins().await;
    let agent = std::path::Path::new(&fixture.spec.cwd).join("agent");
    let path = agent.join("models.yml");
    let mut models: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let model = &mut models["providers"]["ait-local-fixture"]["models"][0];
    model["reasoning"] = json!(true);
    model["thinking"] = json!({"mode":"effort","efforts":["low","high"]});
    std::fs::write(&path, models.to_string()).unwrap();
    std::fs::write(agent.join("config.yml"), json!({"defaultThinkingLevel":"high",
        "modelRoles":{"default":"ait-local-fixture/fixture","smol":"ait-local-fixture/fixture"},
        "modelPresets":{"parent":{"modelRoles":{"default":"ait-local-fixture/fixture"},"defaultThinkingLevel":"low"}}
    }).to_string()).unwrap();
    fixture.spec.config.mode_id = Some("full".to_owned());
    fixture.spec.config.feature_values = Some(BTreeMap::from([(
        "modelPreset".to_owned(),
        json!("parent"),
    )]));
    fixture.spec.config = fixture
        .client
        .resolve_selection(&fixture.spec)
        .await
        .unwrap();
    assert_eq!(
        fixture.spec.config.feature_values.as_ref().unwrap()["modelPresetThinking"],
        "low"
    );
    for (selector, expected) in [("native", "high"), ("ait-local-fixture/fixture:low", "low")] {
        fixture
            .spec
            .config
            .feature_values
            .as_mut()
            .unwrap()
            .insert("modelRole.smol".to_owned(), json!(selector));
        let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
        session
            .out_of_band(&AgentPrompt::text("/vibe"))
            .await
            .unwrap();
        let receipt = control(
            session.as_mut(),
            json!({"operation":"spawn","cli":"fast","prompt":"AIT_VIBE_THINKING_FIXTURE"}),
        )
        .await;
        let id = receipt["session"].as_str().unwrap();
        worker_done(session.as_mut(), id).await;
        let listed = control(session.as_mut(), json!({"operation":"list"})).await;
        assert_eq!(
            listed["workers"][0]["answer"],
            format!("Native worker thinking: {expected}")
        );
        session.close().await.unwrap();
    }
}
