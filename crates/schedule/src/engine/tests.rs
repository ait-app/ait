use super::*;
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Default)]
pub(crate) struct Memory(pub Arc<Mutex<(Vec<Schedule>, bool)>>);
impl Store for Memory {
    fn load(&self) -> Result<Vec<Schedule>, Error> {
        Ok(self.0.lock().unwrap().0.clone())
    }
    fn save(&mut self, records: &[Schedule]) -> Result<(), Error> {
        let mut state = self.0.lock().unwrap();
        if state.1 {
            return Err(Error::Storage);
        }
        state.0 = records.to_vec();
        Ok(())
    }
}
fn now() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().unwrap()
}
fn fixture() -> (Engine, Memory) {
    let store = Memory::default();
    (Engine::open(Box::new(store.clone()), now()).unwrap(), store)
}
pub(crate) fn input() -> Value {
    json!({"workspaceId":"wks-test","name":" test ","prompt":" hello ","cadence":{"type":"every","everyMs":60_000},"target":{"type":"self","agentId":"11111111-1111-4111-8111-111111111111"}})
}
fn create(engine: &mut Engine) -> String {
    engine
        .request("schedule.create.request", input(), now())
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[test]
fn crud_summary_logs_pause_resume_and_delete() {
    let (mut engine, _) = fixture();
    let id = create(&mut engine);
    let listed = engine
        .request("schedule.list.request", json!({}), now())
        .unwrap();
    assert_eq!(listed["schedules"][0]["name"], "test");
    assert!(listed["schedules"][0].get("runs").is_none());
    assert_eq!(listed["schedules"][0]["target"]["type"], "agent");
    let inspect = engine
        .request("schedule.inspect.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert_eq!(inspect["schedule"]["runs"], json!([]));
    let mut full = inspect["schedule"].clone();
    full.as_object_mut().unwrap().remove("runs");
    assert_eq!(listed["schedules"][0], full);
    assert_eq!(
        engine
            .request("schedule.logs.request", json!({"scheduleId":id}), now())
            .unwrap()["runs"],
        json!([])
    );
    engine
        .request("schedule.pause.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert!(engine.due(now()).unwrap().is_empty());
    engine
        .request("schedule.resume.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert_eq!(
        engine.inspect(&id).unwrap().next_run_at,
        Some(now() + chrono::Duration::minutes(1))
    );
    engine
        .request("schedule.delete.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert!(matches!(engine.inspect(&id), Err(Error::NotFound)));
    engine
        .request("schedule.delete.request", json!({"scheduleId":id}), now())
        .unwrap();
}
#[test]
fn failed_writes_do_not_publish_mutation() {
    let (mut engine, store) = fixture();
    let id = create(&mut engine);
    store.0.lock().unwrap().1 = true;
    assert_eq!(
        engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"prompt":"changed"}),
                now()
            )
            .unwrap_err(),
        Error::Storage
    );
    assert_eq!(engine.inspect(&id).unwrap().prompt, "hello");
    assert!(engine.begin(&id, true, now()).is_err());
    assert!(engine.inspect(&id).unwrap().runs.is_empty());
}
#[test]
fn manual_paused_execution_obeys_run_limits_and_expiry() {
    let (mut engine, _) = fixture();
    let id = create(&mut engine);
    engine
        .request("schedule.pause.request", json!({"scheduleId":id}), now())
        .unwrap();
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"maxRuns":1}),
            now(),
        )
        .unwrap();
    let (_, run) = engine.begin(&id, true, now()).unwrap();
    assert_eq!(engine.begin(&id, true, now()).unwrap_err(), Error::Conflict);
    engine
        .finish(&id, &run, true, Outcome::default(), now())
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().status, Status::Completed);
    assert!(engine.inspect(&id).unwrap().next_run_at.is_none());
    assert_eq!(engine.begin(&id, true, now()).unwrap_err(), Error::Conflict);
    let expired = create(&mut engine);
    engine
        .request(
            "schedule.pause.request",
            json!({"scheduleId":expired}),
            now(),
        )
        .unwrap();
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":expired,"expiresAt":now()}),
            now(),
        )
        .unwrap();
    assert_eq!(
        engine.begin(&expired, true, now()).unwrap_err(),
        Error::Conflict
    );
    assert!(engine.inspect(&expired).unwrap().runs.is_empty());
}
#[test]
fn failures_count_towards_max_runs_and_target_gone_completes() {
    let (mut engine, _) = fixture();
    let id = create(&mut engine);
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"maxRuns":1}),
            now(),
        )
        .unwrap();
    let (_, run) = engine.begin(&id, false, now()).unwrap();
    engine
        .finish(
            &id,
            &run,
            false,
            Outcome {
                error: Some("failed".into()),
                ..Outcome::default()
            },
            now(),
        )
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().status, Status::Completed);
    assert!(engine.begin(&id, true, now()).is_err());
    assert!(
        engine
            .request("schedule.resume.request", json!({"scheduleId":id}), now())
            .is_err()
    );
    let id = create(&mut engine);
    let (_, run) = engine.begin(&id, true, now()).unwrap();
    engine
        .finish(
            &id,
            &run,
            true,
            Outcome {
                target_gone: true,
                error: Some("gone".into()),
                ..Outcome::default()
            },
            now(),
        )
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().status, Status::Completed);
}
#[test]
fn expiry_and_cron_defaults() {
    let (mut engine, _) = fixture();
    let mut params = input();
    params["expiresAt"] = json!(now());
    assert_eq!(
        engine
            .request("schedule.create.request", params, now())
            .unwrap_err(),
        Error::Invalid
    );
    assert!(engine.records.is_empty());
    let mut params = input();
    params["cadence"] = json!({"type":"cron","expression":"* * * * *"});
    let created = engine
        .request("schedule.create.request", params.clone(), now())
        .unwrap();
    assert_eq!(
        created["schedule"]["nextRunAt"],
        json!(now() + chrono::Duration::minutes(1))
    );
    params["runOnCreate"] = json!(true);
    let created = engine
        .request("schedule.create.request", params, now())
        .unwrap();
    assert_eq!(created["schedule"]["nextRunAt"], json!(now()));
}
#[test]
fn in_flight_edits_and_pause_survive_completion_and_deletion_waits() {
    let (mut engine, _) = fixture();
    let id = create(&mut engine);
    let (_, run) = engine.begin(&id, false, now()).unwrap();
    engine
        .request("schedule.pause.request", json!({"scheduleId":id}), now())
        .unwrap();
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"name":"new"}),
            now(),
        )
        .unwrap();
    engine
        .finish(&id, &run, false, Outcome::default(), now())
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().status, Status::Paused);
    assert_eq!(engine.inspect(&id).unwrap().name.as_deref(), Some("new"));
    let (_, run) = engine.begin(&id, true, now()).unwrap();
    assert_eq!(
        engine
            .request("schedule.delete.request", json!({"scheduleId":id}), now())
            .unwrap_err(),
        Error::Conflict
    );
    engine
        .finish(&id, &run, true, Outcome::default(), now())
        .unwrap();
    engine
        .request("schedule.delete.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert!(engine.records.is_empty());
}
#[test]
fn restart_marks_interrupted_failed_and_skips_missed_slots() {
    let (mut engine, store) = fixture();
    let id = create(&mut engine);
    let (_, run_id) = engine.begin(&id, false, now()).unwrap();
    let (reply, _) = tokio::sync::oneshot::channel();
    engine
        .checkpoint(&crate::ports::Checkpoint {
            schedule_id: id.clone(),
            run_id,
            agent_id: Some("agent".into()),
            workspace_id: Some("workspace".into()),
            reply,
        })
        .unwrap();
    let recovered = Engine::open(Box::new(store), now() + chrono::Duration::minutes(3))
        .unwrap()
        .inspect(&id)
        .unwrap();
    assert_eq!(recovered.runs[0].status, RunStatus::Failed);
    assert_eq!(recovered.runs[0].workspace_id.as_deref(), Some("workspace"));
    assert_eq!(
        recovered.next_run_at,
        Some(now() + chrono::Duration::minutes(4))
    );
}
#[test]
fn update_inherits_cron_zone_and_clears_nullable_fields() {
    let (mut engine, _) = fixture();
    let mut params = input();
    params["target"] =
        json!({"type":"new-agent","config":{"provider":"codex","cwd":"/tmp","model":"old"}});
    params["cadence"] = json!({"type":"cron","expression":"0 9 * * *","timezone":"Asia/Shanghai"});
    let id = engine
        .request("schedule.create.request", params, now())
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let updated=engine.request("schedule.update.request",json!({"scheduleId":id,"name":null,"maxRuns":null,"expiresAt":null,"cadence":{"type":"cron","expression":"0 10 * * *"},"newAgentConfig":{"model":null,"modeId":"plan"}}),now()).unwrap();
    assert_eq!(updated["schedule"]["cadence"]["timezone"], "Asia/Shanghai");
    assert!(
        updated["schedule"]["target"]["config"]
            .get("model")
            .is_none()
    );
    assert_eq!(updated["schedule"]["name"], Value::Null);
}
#[test]
fn native_schedule_rejects_combined_provider_without_saving() {
    let (mut engine, store) = fixture();
    let mut params = input();
    params["target"] = json!({"type":"new-agent","config":{"provider":"codex/gpt-5"}});
    assert!(matches!(
        engine.request("schedule.create.request", params.clone(), now()),
        Err(Error::Invalid)
    ));
    assert!(store.load().unwrap().is_empty());

    params["target"]["config"] = json!({"provider":"codex","model":"openai/gpt-5"});
    let created = engine
        .request("schedule.create.request", params, now())
        .unwrap();
    let id = &created["schedule"]["id"];
    let before = serde_json::to_value(store.load().unwrap()).unwrap();
    assert!(matches!(
        engine.request(
            "schedule.update.request",
            json!({"scheduleId":id,"newAgentConfig":{"provider":"codex/gpt-5"}}),
            now(),
        ),
        Err(Error::Invalid)
    ));
    assert_eq!(serde_json::to_value(store.load().unwrap()).unwrap(), before);
    let inspected = engine
        .request("schedule.inspect.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert_eq!(
        inspected["schedule"]["target"],
        created["schedule"]["target"]
    );
}

#[test]
fn provider_patch_resets_omitted_provider_settings_only_when_provider_changes() {
    for (patch, provider, model, mode, thinking) in [
        (
            json!({"cwd":"/repo"}),
            "codex",
            Some("original-model"),
            Some("auto"),
            Some("minimal"),
        ),
        (
            json!({"provider":" codex "}),
            "codex",
            Some("original-model"),
            Some("auto"),
            Some("minimal"),
        ),
        (
            json!({"provider":"claude","modeId":null}),
            "claude",
            None,
            None,
            None,
        ),
        (
            json!({"provider":"claude","modeId":null,"model":"replacement"}),
            "claude",
            Some("replacement"),
            None,
            None,
        ),
        (
            json!({"provider":"claude","modeId":null,"model":null}),
            "claude",
            None,
            None,
            None,
        ),
        (
            json!({"provider":"claude","modeId":"plan"}),
            "claude",
            None,
            Some("plan"),
            None,
        ),
        (
            json!({"provider":"claude","modeId":null}),
            "claude",
            None,
            None,
            None,
        ),
        (
            json!({"provider":"claude","modeId":null,"thinkingOptionId":"off"}),
            "claude",
            None,
            None,
            Some("off"),
        ),
        (
            json!({"provider":"claude","modeId":null,"thinkingOptionId":null}),
            "claude",
            None,
            None,
            None,
        ),
    ] {
        let (mut engine, _) = fixture();
        let mut params = input();
        params["target"] = json!({"type":"new-agent","config":{
            "provider":"codex","model":"original-model","modeId":"auto",
            "thinkingOptionId":"minimal"
        }});
        let created = engine
            .request("schedule.create.request", params, now())
            .unwrap();
        let id = &created["schedule"]["id"];

        let updated = engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"newAgentConfig":patch}),
                now(),
            )
            .unwrap();

        let config = &updated["schedule"]["target"]["config"];
        assert_eq!(config["provider"], provider, "{patch}");
        assert_eq!(
            config.get("model").and_then(Value::as_str),
            model,
            "{patch}"
        );
        assert_eq!(
            config.get("modeId").and_then(Value::as_str),
            mode,
            "{patch}"
        );
        assert_eq!(
            config.get("thinkingOptionId").and_then(Value::as_str),
            thinking,
            "{patch}"
        );
    }
}

#[test]
fn invalid_inputs_never_write() {
    let (mut engine, _) = fixture();
    for patch in [
        json!({"prompt":" "}),
        json!({"maxRuns":0}),
        json!({"cadence":{"type":"every","everyMs":0}}),
        json!({"target":{"type":"agent","agentId":"bad"}}),
        json!({"surprise":true}),
    ] {
        let mut params = input();
        params
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(
            engine
                .request("schedule.create.request", params, now())
                .is_err()
        );
    }
    assert!(engine.records.is_empty());
    assert!(
        engine
            .request("schedule.list.request", json!({"extra":true}), now())
            .is_err()
    );
}

mod lifecycle;
mod mutation;
mod recovery;

mod workspace;

#[test]
fn model_patch_resets_omitted_thinking_only_when_model_changes() {
    for (patch, model, thinking) in [
        (json!({"model":"original"}), Some("original"), Some("max")),
        (json!({"model":"replacement"}), Some("replacement"), None),
        (json!({"model":null}), None, None),
        (json!({"model":""}), None, None),
        (
            json!({"model":"replacement","thinkingOptionId":"high"}),
            Some("replacement"),
            Some("high"),
        ),
        (
            json!({"model":"replacement","thinkingOptionId":null}),
            Some("replacement"),
            None,
        ),
    ] {
        let (mut engine, _) = fixture();
        let mut params = input();
        params["target"] = json!({"type":"new-agent","config":{
            "provider":"omp","model":"original","modeId":"auto","thinkingOptionId":"max"
        }});
        let created = engine
            .request("schedule.create.request", params, now())
            .unwrap();
        let updated = engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":created["schedule"]["id"],"newAgentConfig":patch}),
                now(),
            )
            .unwrap();
        let config = &updated["schedule"]["target"]["config"];
        assert_eq!(config["provider"], "omp", "{patch}");
        assert_eq!(config["modeId"], "auto", "{patch}");
        assert_eq!(
            config.get("model").and_then(Value::as_str),
            model,
            "{patch}"
        );
        assert_eq!(
            config.get("thinkingOptionId").and_then(Value::as_str),
            thinking,
            "{patch}"
        );
    }
}

#[test]
fn loaded_legacy_provider_allows_updates_without_resupplying_provider() {
    for (patch, provider, model, thinking) in [
        (
            json!({"prompt":"updated"}),
            "codex/gpt-5",
            None,
            Some("high"),
        ),
        (
            json!({"cadence":{"type":"cron","expression":"0 10 * * *"}}),
            "codex/gpt-5",
            None,
            Some("high"),
        ),
        (
            json!({"newAgentConfig":{"model":"replacement"}}),
            "codex",
            Some("replacement"),
            None,
        ),
        (
            json!({"newAgentConfig":{"model":null}}),
            "codex",
            None,
            None,
        ),
        (
            json!({"newAgentConfig":{"model":"gpt-5"}}),
            "codex",
            Some("gpt-5"),
            Some("high"),
        ),
        (
            json!({"newAgentConfig":{"cwd":"/tmp"}}),
            "codex",
            Some("gpt-5"),
            Some("high"),
        ),
        (
            json!({"newAgentConfig":{"provider":"codex"}}),
            "codex",
            Some("gpt-5"),
            Some("high"),
        ),
    ] {
        let (mut engine, store) = fixture();
        let id = create(&mut engine);
        store.0.lock().unwrap().0[0].target = Target::NewAgent {
            config: json!({"provider":"codex/gpt-5","thinkingOptionId":"high"}),
        };
        let mut engine = Engine::open(Box::new(store.clone()), now()).unwrap();
        let mut params = patch.clone();
        params["scheduleId"] = json!(id);
        let updated = engine
            .request("schedule.update.request", params, now())
            .unwrap();
        let schedule = &updated["schedule"];
        if let Some(config) = patch.get("newAgentConfig") {
            for (key, value) in config.as_object().unwrap() {
                assert_eq!(&schedule["target"]["config"][key], value);
            }
        } else {
            for (key, value) in patch.as_object().unwrap() {
                assert_eq!(&schedule[key], value);
            }
        }
        let config = &schedule["target"]["config"];
        assert_eq!(config["provider"], provider, "{patch}");
        assert_eq!(
            config.get("model").and_then(Value::as_str),
            model,
            "{patch}"
        );
        assert_eq!(
            config.get("thinkingOptionId").and_then(Value::as_str),
            thinking,
            "{patch}"
        );
        assert_eq!(
            serde_json::to_value(&store.load().unwrap()[0]).unwrap(),
            *schedule
        );
    }
}

#[test]
fn clearing_absent_model_preserves_thinking_option() {
    for model in [Value::Null, json!(""), json!("  ")] {
        let (mut engine, store) = fixture();
        let mut params = input();
        params["target"] = json!({"type":"new-agent","config":{
            "provider":"omp","modeId":"auto","thinkingOptionId":"max"
        }});
        let created = engine
            .request("schedule.create.request", params, now())
            .unwrap();
        let updated = engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":created["schedule"]["id"],"newAgentConfig":{"model":model}}),
                now(),
            )
            .unwrap();
        assert_eq!(updated["schedule"]["target"], created["schedule"]["target"]);
        assert_eq!(
            serde_json::to_value(&store.load().unwrap()[0]).unwrap(),
            updated["schedule"]
        );
    }
}

#[test]
fn provider_switch_requires_explicit_mode_without_saving() {
    let (mut engine, store) = fixture();
    let mut params = input();
    params["target"] = json!({"type":"new-agent","config":{
        "provider":"codex","model":"gpt-5","modeId":"auto-review"
    }});
    let created = engine
        .request("schedule.create.request", params, now())
        .unwrap();
    let before = serde_json::to_value(store.load().unwrap()).unwrap();
    assert!(
        engine
            .request(
                "schedule.update.request",
                json!({
                    "scheduleId":created["schedule"]["id"],"newAgentConfig":{"provider":"omp"}
                }),
                now()
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(store.load().unwrap()).unwrap(), before);
}

#[test]
fn repairing_legacy_provider_preserves_embedded_model() {
    let mut target = Target::NewAgent {
        config: json!({
            "provider":"codex/openai/gpt-5","modeId":"auto-review"
        }),
    };
    patch_new_agent_config(&mut target, &json!({"provider":" codex "})).unwrap();
    let Target::NewAgent { config } = target else {
        panic!("new-agent target")
    };
    assert_eq!(
        config,
        json!({"provider":"codex","model":"openai/gpt-5","modeId":"auto-review"})
    );
}

#[test]
fn legacy_config_patch_preserves_existing_separate_model() {
    let mut target = Target::NewAgent {
        config: json!({
            "provider":"codex/openai/gpt-5","model":"replacement","thinkingOptionId":"high"
        }),
    };
    patch_new_agent_config(&mut target, &json!({"cwd":"/tmp"})).unwrap();
    let Target::NewAgent { config } = target else {
        panic!("new-agent target")
    };
    assert_eq!(
        config,
        json!({
            "provider":"codex","model":"replacement","thinkingOptionId":"high","cwd":"/tmp"
        })
    );
}

#[test]
fn replacing_target_with_legacy_provider_is_rejected_without_saving() {
    let (mut engine, store) = fixture();
    let id = create(&mut engine);
    store.0.lock().unwrap().0[0].target = Target::NewAgent {
        config: json!({"provider":"codex/gpt-5"}),
    };
    let mut engine = Engine::open(Box::new(store.clone()), now()).unwrap();
    let before = store.load().unwrap();
    assert!(
        engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"target":before[0].target}),
                now(),
            )
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(store.load().unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
}
