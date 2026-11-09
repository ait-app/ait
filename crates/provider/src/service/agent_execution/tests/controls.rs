use super::*;

#[tokio::test]
async fn controls_use_each_sessions_native_modes_and_preserve_static_fallback() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let other = create(&execution, &fixture).await;
    for modes in [
        json!([{"id":"review","label":"Review"}]),
        json!([]),
        json!("malformed"),
    ] {
        registry
            .update(id, &|current| {
                let mut next = current.clone();
                next.runtime_info = Some(
                    serde_json::from_value(json!({
                        "provider":"codex","extra":{"availableModes":modes}
                    }))
                    .unwrap(),
                );
                next
            })
            .unwrap();
        let snapshot = execution
            .execute("agent.get.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(
            snapshot["agent"]["availableModes"],
            if modes.is_array() {
                &modes
            } else {
                &created["agent"]["availableModes"]
            }
            .clone()
        );
        let snapshot = execution
            .execute("agent.get.request", json!({"agentId":other["agentId"]}))
            .await
            .unwrap();
        assert_eq!(
            snapshot["agent"]["availableModes"],
            other["agent"]["availableModes"]
        );
    }
    execution.shutdown().await.unwrap();
}

pub(super) async fn pending(execution: &AgentExecution, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = execution
                .execute("agent.get.request", json!({"agentId":id}))
                .await
                .unwrap();
            if !snapshot["agent"]["pendingPermissions"]
                .as_array()
                .unwrap()
                .is_empty()
            {
                return snapshot["agent"]["pendingPermissions"][0].clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn native_permissions_are_agent_scoped_ephemeral_and_support_questions() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let other = create(&execution, &fixture).await;
    for (text, response) in [
        ("permit-command", json!({"behavior":"allow"})),
        ("permit-file", json!({"behavior":"deny"})),
        (
            "permit-question",
            json!({"behavior":"allow","updatedInput":{"answers":{"choice":["first"]}}}),
        ),
    ] {
        execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":text}),
            )
            .await
            .unwrap();
        let request = pending(&execution, id).await;
        assert_eq!(
            registry.get(id).unwrap().unwrap().attention_reason,
            Some(domain::agent_runtime::AgentAttentionReason::Permission)
        );
        let request_id = &request["id"];
        assert!(
            execution
                .execute(
                    "agent.permission.resolve.request",
                    json!({"agentId":other["agentId"],"requestId":request_id,"response":response})
                )
                .await
                .is_err()
        );
        assert!(execution.execute("agent.permission.resolve.request",json!({"agentId":id,"requestId":request_id,"response":{"behavior":"allow","updatedPermissions":[]}})).await.is_err());
        execution
            .execute(
                "agent.permission.resolve.request",
                json!({"agentId":id,"requestId":request_id,"response":response}),
            )
            .await
            .unwrap();
        assert!(
            execution
                .execute(
                    "agent.permission.resolve.request",
                    json!({"agentId":id,"requestId":request_id,"response":response})
                )
                .await
                .is_err()
        );
        assert_eq!(
            execution
                .execute("agent.finish.wait.request", json!({"agentId":id}))
                .await
                .unwrap()["status"],
            "idle"
        );
    }
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let request = pending(&execution, id).await;
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(
        execution
            .execute(
                "agent.permission.resolve.request",
                json!({"agentId":id,"requestId":request["id"],"response":{"behavior":"allow"}})
            )
            .await
            .is_err()
    );
    assert_eq!(
        execution
            .execute("agent.get.request", json!({"agentId":id}))
            .await
            .unwrap()["agent"]["pendingPermissions"],
        json!([])
    );
    execution.shutdown().await.unwrap();
}

fn assert_created_controls(created: &Value) {
    let modes = created["agent"]["availableModes"].as_array().unwrap();
    assert_eq!(modes.len(), 2);
    assert_eq!(modes[0]["id"], "auto");
    assert_eq!(modes[1]["id"], "full-access");
    assert_eq!(modes[1]["icon"], "ShieldOff");
    assert_eq!(created["agent"]["features"][0]["id"], "service_tier");
    assert_eq!(created["agent"]["features"][0]["icon"], "zap");
    assert_eq!(created["agent"]["features"][0]["value"], "default");
    assert_eq!(
        created["agent"]["capabilities"]["supportsRewindConversation"],
        true
    );
}

#[tokio::test]
async fn controls_validate_atomically_apply_next_turn_and_persist() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    assert_created_controls(&created);
    let commands = execution
        .execute("agent.commands.list.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_has_review(&commands);
    let draft = execution
        .execute(
            "agent.commands.list.request",
            json!({"agentId":"draft","draftConfig":{"provider":"codex","cwd":fixture.cwd}}),
        )
        .await
        .unwrap();
    assert_eq!(draft["commands"], commands["commands"]);
    assert_eq!(
        execution
            .execute(
                "agent.mode.set.request",
                json!({"agentId":id,"modeId":"auto"})
            )
            .await
            .unwrap()["accepted"],
        true
    );
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    let changed = execution.execute("agent.config.apply.request",json!({"agentId":id,"config":{"modeId":"full-access","featureValues":{"fast_mode":true}}})).await.unwrap();
    assert_eq!(changed["accepted"], true);
    assert_eq!(changed["notice"]["type"], "warning");
    assert_rejected_controls(&execution, &registry, id).await;
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
    let (execution, _) = worker(&fixture);
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"after restart"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let last = fixture
        .requests()
        .into_iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(last["params"]["sandboxPolicy"]["type"], "dangerFullAccess");
    assert_eq!(last["params"]["serviceTier"], "fast");
    assert_eq!(
        execution
            .execute(
                "agent.feature.set.request",
                json!({"agentId":id,"featureId":"fast_mode","value":false})
            )
            .await
            .unwrap()["accepted"],
        true
    );
    fixture.mode("no-fast");
    assert_eq!(
        execution
            .execute(
                "agent.feature.set.request",
                json!({"agentId":id,"featureId":"fast_mode","value":true})
            )
            .await
            .unwrap()["accepted"],
        false
    );
    execution.shutdown().await.unwrap();
}

fn seed(fixture: &Fixture, id: &str, parent: Option<&str>) {
    std::fs::write(
        fixture.cwd.join(format!("native-session-{id}.json")),
        json!({"id":id,"cwd":fixture.cwd,"parentThreadId":parent}).to_string(),
    )
    .unwrap();
    std::fs::write(fixture.cwd.join(format!("native-history-{id}.json")),json!([{"id":"turn","status":"completed","items":[{"id":"user","type":"userMessage","content":[{"type":"text","text":"child prompt"}]}]}]).to_string()).unwrap();
}

#[tokio::test]
async fn subagent_history_is_scoped_to_verified_descendants_and_registers_read_only_agents() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let root = created["agent"]["persistence"]["sessionId"]
        .as_str()
        .unwrap();
    seed(&fixture, "child", Some(root));
    seed(&fixture, "grandchild", Some("child"));
    seed(&fixture, "foreign", Some("other"));
    let listed = execution
        .execute(
            "agent.provider_subagents.list.request",
            json!({"parentAgentId":id}),
        )
        .await
        .unwrap();
    assert_eq!(listed["subagents"].as_array().unwrap().len(), 2);
    assert_eq!(listed["subagents"][0]["parentSubagentId"], Value::Null);
    assert_eq!(listed["subagents"][1]["parentSubagentId"], "child");
    let page = execution
        .execute(
            "agent.provider_subagents.timeline.get.request",
            json!({"parentAgentId":id,"subagentId":"grandchild","limit":1}),
        )
        .await
        .unwrap();
    assert_eq!(page["rows"][0]["item"]["text"], "child prompt");
    assert_eq!(page["rows"][0]["seq"], 1);
    let empty = execution
        .execute(
            "agent.provider_subagents.timeline.get.request",
            json!({"parentAgentId":id,"subagentId":"grandchild","cursor":page["endCursor"]}),
        )
        .await
        .unwrap();
    assert_eq!(empty["rows"], json!([]));
    assert!(
        execution
            .execute(
                "agent.provider_subagents.timeline.get.request",
                json!({"parentAgentId":id,"subagentId":"foreign"})
            )
            .await
            .is_err()
    );
    assert_eq!(registry.list().unwrap().len(), 3);
    execution.shutdown().await.unwrap();
}

#[test]
fn background_children_keep_running_after_the_parent_turn_and_share_normal_history() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = Fixture::new();
            let (execution, registry) = worker(&fixture);
            let created = create(&execution, &fixture).await;
            let root = created["agentId"].as_str().unwrap();
            execution
                .execute(
                    "agent.message.send.request",
                    json!({"agentId":root,"text":"background-subagent"}),
                )
                .await
                .unwrap();
            execution
                .execute("agent.finish.wait.request", json!({"agentId":root}))
                .await
                .unwrap();
            let listed = execution
                .execute(
                    "agent.subagents.list.request",
                    json!({"parentAgentId":root}),
                )
                .await
                .unwrap();
            let child = listed["agents"][0]["id"].as_str().unwrap();
            let native = listed["agents"][0]["observation"]["nativeId"]
                .as_str()
                .unwrap();
            assert_eq!(listed["agents"][0]["status"], "running");
            assert_eq!(
                registry.get(root).unwrap().unwrap().last_status,
                domain::agent_runtime::AgentRuntimeStatus::Idle
            );
            let initial = execution
                .execute("agent.timeline.get.request", json!({"agentId":child}))
                .await
                .unwrap();
            assert_eq!(initial["entries"][0]["item"]["text"], "Independent child");
            std::fs::write(fixture.cwd.join(format!("release-{native}")), "finish").unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let record = registry.get(child).unwrap().unwrap();
                    if record.observation.as_ref().unwrap().status
                        == domain::agent_runtime::observation::ObservedStatus::Completed
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let page = execution
                .execute("agent.timeline.get.request", json!({"agentId":child}))
                .await
                .unwrap();
            let answer = page["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|row| row["item"]["text"].as_str())
                .collect::<String>();
            assert_eq!(answer, "Independent child answer");
            assert_eq!(page["agent"]["observation"]["status"], "completed");
            assert_eq!(page["agent"]["capabilities"]["supportsArchive"], true);
            execution
                .execute("agent.archive.request", json!({"agentId":child}))
                .await
                .unwrap();
            assert!(registry.get(root).unwrap().unwrap().archived_at.is_none());
            assert!(
                fixture
                    .cwd
                    .join(format!("native-history-{native}.json"))
                    .exists()
            );
            execution.shutdown().await.unwrap();
        });
}

#[test]
fn unified_children_share_normal_reads_and_reject_independent_writers() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(async {
            let fixture = Fixture::new();
            let (execution, registry) = worker(&fixture);
            let created = create(&execution, &fixture).await;
            let root = created["agentId"].as_str().unwrap();
            let native = created["agent"]["persistence"]["sessionId"]
                .as_str()
                .unwrap();
            seed(&fixture, "child", Some(native));
            seed(&fixture, "grandchild", Some("child"));
            seed(&fixture, "foreign", Some("other"));
            let listed = execution
                .execute(
                    "agent.subagents.list.request",
                    json!({"parentAgentId":root}),
                )
                .await
                .unwrap();
            assert_eq!(listed["agents"].as_array().unwrap().len(), 1);
            let child = listed["agents"][0]["id"].as_str().unwrap();
            assert_eq!(listed["agents"][0]["management"], "observed");
            assert_eq!(listed["agents"][0]["parentAgentId"], root);
            assert_eq!(listed["agents"][0]["persistence"], Value::Null);
            assert_eq!(
                listed["agents"][0]["capabilities"]["supportsMessageSend"],
                false
            );
            assert_eq!(listed["agents"][0]["observation"]["status"], "unknown");
            let nested = execution
                .execute(
                    "agent.subagents.list.request",
                    json!({"parentAgentId":child}),
                )
                .await
                .unwrap();
            assert_eq!(nested["agents"].as_array().unwrap().len(), 1);
            assert_eq!(nested["agents"][0]["parentAgentId"], child);
            let legacy = execution
                .execute(
                    "agent.provider_subagents.timeline.get.request",
                    json!({"parentAgentId":root,"subagentId":"child"}),
                )
                .await
                .unwrap();
            let page = execution
                .execute("agent.timeline.get.request", json!({"agentId":child}))
                .await
                .unwrap();
            assert_eq!(page["agentId"], child);
            assert_eq!(page["entries"][0]["item"]["text"], "child prompt");
            assert_eq!(page["epoch"], legacy["epoch"]);
            assert_eq!(page["endCursor"], legacy["endCursor"]);
            let default = execution
                .execute("agent.list.request", json!({"scope":"active","sync":{}}))
                .await
                .unwrap();
            assert_eq!(default["entries"].as_array().unwrap().len(), 1);
            let modern = execution
                .execute(
                    "agent.list.request",
                    json!({"scope":"active","includeObserved":true,"sync":{"generation":default["sync"]["generation"],"afterSeq":default["sync"]["headSeq"]}}),
                )
                .await
                .unwrap();
            assert_eq!(modern["entries"].as_array().unwrap().len(), 3);
            assert_eq!(modern["sync"]["mode"], "snapshot");
            assert_observed_read_only(&execution, &fixture, &registry, child).await;
            let alias = execution
                .execute(
                    "agent.subagents.list.request",
                    json!({"parentAgentId":root,"subagentId":"grandchild"}),
                )
                .await
                .unwrap();
            assert_eq!(alias["agents"][0]["id"], nested["agents"][0]["id"]);
            execution.shutdown().await.unwrap();
            let (execution, _) = worker(&fixture);
            let reopened = execution
                .execute("agent.get.request", json!({"agentId":child}))
                .await
                .unwrap();
            assert_eq!(reopened["agent"]["observation"]["health"], "stale");
            assert_eq!(reopened["agent"]["status"], "closed");
            execution.shutdown().await.unwrap();
        });
}

async fn assert_observed_read_only(
    execution: &AgentExecution,
    fixture: &Fixture,
    registry: &FileBackedAgentRuntimeRegistry,
    child: &str,
) {
    let original = registry.get(child).unwrap().unwrap();
    let requests = fixture.requests().len();
    for method in [
        "agent.cancel.request",
        "agent.refresh.request",
        "agent.commands.list.request",
        "agent.detach.request",
        "agent.finish.wait.request",
    ] {
        assert!(
            execution
                .execute(method, json!({"agentId":child}))
                .await
                .is_err(),
            "{method}"
        );
    }
    for method in [
        "agent.commands.list.request",
        "agent.commands.complete.request",
    ] {
        let mut params =
            json!({"agentId":child,"draftConfig":{"provider":"codex","cwd":fixture.cwd}});
        if method == "agent.commands.complete.request" {
            params["text"] = json!("/review ");
            params["cursor"] = json!(8);
        }
        assert_eq!(
            execution.execute(method, params).await.unwrap_err(),
            ErrorCode::UnsupportedCapability
        );
    }
    assert!(
        execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":child,"text":"write"})
            )
            .await
            .is_err()
    );
    assert!(
        execution
            .execute(
                "agent.config.apply.request",
                json!({"agentId":child,"config":{"modelId":"other"}})
            )
            .await
            .is_err()
    );
    assert!(
        execution
            .execute(
                "agent.import.request",
                json!({"provider":"codex","sessionId":"child","cwd":fixture.cwd})
            )
            .await
            .is_err()
    );
    assert!(execution.execute("agent.resume.request", json!({"handle":{"provider":"codex","sessionId":"child"},"overrides":{"cwd":fixture.cwd}})).await.is_err());
    assert_eq!(fixture.requests().len(), requests);
    assert_eq!(registry.get(child).unwrap().unwrap(), original);
}

#[tokio::test]
async fn rewind_switches_to_fork_rotates_timeline_and_survives_restart() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let source = created["agent"]["persistence"]["sessionId"]
        .as_str()
        .unwrap();
    for text in ["one", "two"] {
        execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":text}),
            )
            .await
            .unwrap();
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
    }
    let page = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    let original =
        std::fs::read_to_string(fixture.cwd.join(format!("native-history-{source}.json"))).unwrap();
    let turns: Value = serde_json::from_str(&original).unwrap();
    let message = &turns[1]["items"][0]["id"];
    assert_eq!(
        execution
            .execute(
                "agent.rewind.request",
                json!({"agentId":id,"messageId":message,"mode":"files"})
            )
            .await
            .unwrap()["ok"],
        false
    );
    assert_eq!(
        execution
            .execute(
                "agent.rewind.request",
                json!({"agentId":id,"messageId":message,"mode":"conversation"})
            )
            .await
            .unwrap()["ok"],
        true
    );
    assert_ne!(
        registry
            .get(id)
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .session_id,
        source
    );
    assert_eq!(
        std::fs::read_to_string(fixture.cwd.join(format!("native-history-{source}.json"))).unwrap(),
        original
    );
    let next = execution
        .execute(
            "agent.timeline.get.request",
            json!({"agentId":id,"cursor":page["endCursor"]}),
        )
        .await
        .unwrap();
    assert_eq!(next["reset"], true);
    assert_eq!(next["entries"].as_array().unwrap().len(), 2);
    assert_ne!(next["epoch"], page["epoch"]);
    execution.shutdown().await.unwrap();
    let (execution, _) = worker(&fixture);
    let restored = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(restored["epoch"], next["epoch"]);
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"branch"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}

async fn assert_rejected_controls(
    execution: &AgentExecution,
    registry: &FileBackedAgentRuntimeRegistry,
    id: &str,
) {
    let original = registry.get(id).unwrap().unwrap();
    for config in [
        json!({"modeId":"unknown"}),
        json!({"modeId":null}),
        json!({"featureValues":{"plan_mode":true}}),
        json!({"modeId":"auto","featureValues":{"fast_mode":"true"}}),
    ] {
        assert_eq!(
            execution
                .execute(
                    "agent.config.apply.request",
                    json!({"agentId":id,"config":config})
                )
                .await
                .unwrap()["accepted"],
            false
        );
        assert_eq!(registry.get(id).unwrap().unwrap(), original);
    }
    assert!(
        execution
            .execute(
                "agent.config.apply.request",
                json!({"agentId":id,"config":{"featureValues":null}})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn interrupted_rewind_projection_recovers_from_durable_replacement_marker() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"history"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let page = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    let database = rusqlite::Connection::open(fixture.root.path().join("timeline.sqlite")).unwrap();
    database.execute_batch("CREATE TRIGGER reject_rewind BEFORE INSERT ON retired_entries BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let failed = execution.execute("agent.rewind.request",json!({"agentId":id,"messageId":page["entries"][0]["item"]["messageId"],"mode":"conversation"})).await.unwrap();
    assert_eq!(failed["ok"], false);
    let record = registry.get(id).unwrap().unwrap();
    assert_ne!(
        record.persistence.as_ref().unwrap().session_id,
        created["agent"]["persistence"]["sessionId"]
    );
    assert_eq!(
        record
            .persistence
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()["aitTimelineReplacement"],
        true
    );
    assert!(
        execution
            .execute("agent.timeline.get.request", json!({"agentId":id}))
            .await
            .is_err()
    );
    assert!(
        execution
            .execute("agent.resume.request", json!({"agentId":id}))
            .await
            .is_err()
    );
    execution.shutdown().await.unwrap();
    database
        .execute_batch("DROP TRIGGER reject_rewind;")
        .unwrap();
    let (execution, registry) = worker(&fixture);
    let recovered = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(recovered["entries"], json!([]));
    assert_ne!(recovered["epoch"], page["epoch"]);
    assert!(
        !registry
            .get(id)
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .metadata
            .unwrap()
            .contains_key("aitTimelineReplacement")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn worker_restart_clears_permission_attention_without_reusing_request_ids() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let previous = pending(&execution, id).await;
    execution.shutdown().await.unwrap();
    registry
        .update(id, &|current| {
            let mut next = current.clone();
            next.requires_attention = true;
            next.attention_reason = Some(domain::agent_runtime::AgentAttentionReason::Permission);
            next.attention_timestamp = Some("2026-09-25T00:00:00Z".to_owned());
            next
        })
        .unwrap();
    let (execution, registry) = worker(&fixture);
    assert!(!registry.get(id).unwrap().unwrap().requires_attention);
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let current = pending(&execution, id).await;
    assert_ne!(current["id"], previous["id"]);
    assert!(
        execution
            .execute(
                "agent.permission.resolve.request",
                json!({"agentId":id,"requestId":previous["id"],"response":{"behavior":"allow"}})
            )
            .await
            .is_err()
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn denied_permission_is_resolved_when_completion_races_optional_interrupt() {
    let fixture = Fixture::new();
    fixture.mode("interrupt-completed");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let permission = pending(&execution, id).await;
    execution.execute("agent.permission.resolve.request",json!({"agentId":id,"requestId":permission["id"],"response":{"behavior":"deny","interrupt":true}})).await.unwrap();
    assert_eq!(
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap()["status"],
        "idle"
    );
    assert_eq!(
        execution
            .execute("agent.get.request", json!({"agentId":id}))
            .await
            .unwrap()["agent"]["pendingPermissions"],
        json!([])
    );
    execution.shutdown().await.unwrap();
}

fn assert_has_review(commands: &Value) {
    assert!(
        commands["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "review")
    );
}

#[tokio::test]
async fn completion_rejects_invalid_input_and_reports_unsupported_without_prompt_submission() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let completion = execution
        .execute(
            "agent.commands.complete.request",
            json!({
                "agentId":id, "text":"/review ", "cursor":8
            }),
        )
        .await
        .unwrap();
    assert_eq!(completion["supported"], false);
    assert_eq!(completion["items"], json!([]));
    assert_eq!(
        execution
            .execute(
                "agent.commands.complete.request",
                json!({
                    "agentId":id, "text":"/review ", "cursor":99
                })
            )
            .await
            .unwrap_err(),
        ErrorCode::InvalidMessage
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn draft_completion_accepts_unknown_agent_without_creating_or_prompting() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let before = registry.list().unwrap();
    let completion = execution
        .execute(
            "agent.commands.complete.request",
            json!({
                "agentId":"uncreated-draft", "text":"/review ", "cursor":8,
                "draftConfig":{"provider":"codex","cwd":fixture.cwd}
            }),
        )
        .await
        .unwrap();
    assert_eq!(completion["supported"], false);
    assert_eq!(completion["items"], json!([]));
    assert_eq!(registry.list().unwrap(), before);
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    assert_eq!(
        execution
            .execute(
                "agent.commands.complete.request",
                json!({
                    "agentId":"uncreated-draft", "text":"/review ", "cursor":8
                })
            )
            .await
            .unwrap_err(),
        ErrorCode::AgentNotFound
    );
    execution.shutdown().await.unwrap();
}
