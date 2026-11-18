use std::fs;
use std::mem;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Peer, branch_text, env_set, model_route, new_session};

const STEERING_UNAVAILABLE: &str = "Steering is unavailable api_key=fixture-secret";

impl Peer {
    /// Answer one RPC command, returning false when a scenario closes the input loop.
    pub(super) async fn dispatch(&mut self, command: &Value) -> bool {
        let kind = command["type"].as_str().expect("fixture command type");
        if self.pending_dialog.is_some() && matches!(kind, "get_state" | "steer") {
            self.dialog_commands.push(command.clone());
            return true;
        }

        match kind {
            "negotiate_protocol" => {
                self.response(command, json!({"protocolVersion": 2}));
                self.protocol = 2;
            }
            "get_state" => self.get_state(command),
            "fixture_new_context" => self.new_context(command),
            "get_available_models" => {
                self.response(command, json!({"models": self.available}));
            }
            "get_available_thinking_levels" => {
                let efforts = self.model["thinking"]["efforts"]
                    .as_array()
                    .expect("model's available thinking efforts");
                let off = json!("off");
                let levels: Vec<_> = std::iter::once(&off).chain(efforts).collect();
                self.response(command, json!({"levels": levels}));
            }
            "set_model" => self.set_model(command),
            "set_thinking_level" => {
                self.level = command["level"]
                    .as_str()
                    .expect("thinking level")
                    .to_owned();
                self.response(command, Value::Null);
            }
            "set_fast_mode" => self.set_fast_mode(command),
            "set_slow_mode" => {
                let enabled = command["enabled"].as_bool().expect("slow mode flag");
                if self.model["provider"] != "test" && enabled {
                    self.reject(command, "Rejected");
                } else {
                    self.response(command, Value::Null);
                }
            }
            "set_ask_dialog" if env_set("OMP_FIXTURE_REJECT_ASK_DIALOG") => {
                self.reject(command, "Rejected");
            }
            "set_subagent_subscription" if rejects_subscription(command) => {
                self.reject(command, "Rejected");
            }
            "set_auto_compaction" => {
                self.auto_compaction = command["enabled"].as_bool().expect("auto compaction flag");
                self.response(command, Value::Null);
            }
            "get_session_stats" if env_set("OMP_FIXTURE_REJECT_STATS") => {
                self.reject(command, "Unknown command: get_session_stats");
            }
            "get_session_stats" => self.response(command, self.session_stats()),
            "compact" => self.compact(command),
            "handoff" => self.handoff(command),
            "set_auto_retry" | "set_ask_dialog" | "set_subagent_subscription" => {
                self.response(command, Value::Null);
            }
            "get_branch_messages" => self.get_branch_messages(command),
            "get_subagents" => self.response(command, json!({"subagents": []})),
            "get_available_commands" => {
                let mut commands =
                    vec![json!({"name": "review", "description": "Review", "source": "skill"})];
                commands.extend(self.read_plugins());
                self.response(command, json!({"commands": commands}));
            }
            "interactive_control" => {
                self.pending_control = Some(command.clone());
                self.emit(&json!({
                    "type": "extension_ui_request", "id": "control-question",
                    "method": "input", "title": "Native control input",
                }));
            }
            "host_tool_update" => {}
            "host_tool_result" => self.finish(true),
            "prompt"
                if self.active
                    && command["streamingBehavior"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty()) =>
            {
                self.queue_prompt(command);
            }
            "prompt" => return self.prompt(command),
            "steer" => self.steer(command),
            "abort" => self.abort(command).await,
            "extension_ui_response" => self.extension_ui_response(command),
            _ => self.reject(command, "Rejected"),
        }
        true
    }

    fn get_state(&mut self, command: &Value) {
        if self.pending_async && self.root.join("release-settlement").exists() {
            self.pending_async = false;
            self.emit(&json!({"type": "agent_start"}));
            self.assistant("Background finished", "stop");
            self.emit(&json!({
                "type": "agent_end", "messages": [], "isTerminal": true, "yielded": true,
            }));
        }
        let mut state = json!({
            "sessionId": self.session_id,
            "sessionFile": self.file,
            "model": self.model,
            "thinkingLevel": self.level,
            "isStreaming": self.active,
            "isCompacting": false,
            "hasPendingAsyncWork": self.pending_async,
            "queuedMessageCount": 0,
            "slowModeSupported": self.model["provider"] == "test",
            "autoCompactionEnabled": self.auto_compaction,
            "contextUsage": self.context_usage(),
            "todoPhases": self.todo_phases,
        });
        if self.current_prompt != "missing-settlement-fact" {
            state["isSettled"] = json!(!self.active && !self.pending_async);
        }
        // OMP 18.18.8 reports both facts; older releases omit them.
        if !env_set("OMP_FIXTURE_NO_FAST_STATE") {
            state["fastModeEnabled"] = json!(self.fast_enabled);
            state["fastModeActive"] = json!(self.fast_active());
        }
        self.response(command, state);
    }

    fn new_context(&mut self, command: &Value) {
        (self.file, self.records, self.session_id) =
            new_session(&self.root, &self.cwd, "2026-10-25");
        self.response(command, json!({"sessionId": self.session_id}));
    }

    fn set_model(&mut self, command: &Value) {
        let selected = self.available.iter().find(|model| {
            model["provider"] == command["provider"] && model["id"] == command["modelId"]
        });
        if let Some(model) = selected
            && !env_set("OMP_FIXTURE_SET_MODEL_FAILS")
        {
            self.model = model.clone();
            self.entry("model_change", json!({"model": model_route(&self.model)}));
            // OMP 18.18.8 announces a field-less model_changed before answering set_model.
            self.emit(&json!({"type": "model_changed"}));
            self.response(command, self.model.clone());
        } else {
            self.reject(command, "Rejected");
        }
    }

    fn set_fast_mode(&mut self, command: &Value) {
        let enabled = command["enabled"].as_bool().expect("fast mode flag");
        if enabled && self.model["provider"] != "test" {
            self.reject(command, "Fast mode is unavailable for the current model.");
            return;
        }
        self.fast_enabled = enabled;
        // Guessed shape of the persisted tier; OMP 18.18.8 records it per provider.
        let tier = if enabled {
            json!({"test": "priority"})
        } else {
            Value::Null
        };
        self.entry("service_tier_change", json!({"serviceTier": tier}));
        self.response(
            command,
            json!({"enabled": enabled, "active": self.fast_active()}),
        );
    }

    fn handoff(&mut self, command: &Value) {
        // OMP records a successful handoff as a compaction entry without sending an event.
        if self.active {
            self.reject(command, "Cannot hand off while a response is in progress");
        } else if self.current_messages().next().is_none() {
            self.reject(command, "Nothing to hand off (already compacted)");
        } else {
            self.entry(
                "compaction",
                json!({
                    "summary": "Handoff", "firstKeptEntryId": self.leaf, "tokensBefore": 17,
                    "tokensAfter": 100, "method": "handoff",
                }),
            );
            self.persist();
            self.response(command, json!({}));
        }
    }

    fn get_branch_messages(&self, command: &Value) {
        // OMP lists every recorded user message entry with text, in file order.
        let messages: Vec<_> = self
            .records
            .iter()
            .filter(|record| record["type"] == "message" && record["message"]["role"] == "user")
            .filter_map(|record| {
                let text = branch_text(&record["message"]["content"]);
                (!text.is_empty()).then(|| json!({"entryId": record["id"], "text": text}))
            })
            .collect();
        self.response(command, json!({"messages": messages}));
    }

    fn queue_prompt(&mut self, command: &Value) {
        // OMP delivers prompts queued with a streaming behavior during the current run.
        if self.current_prompt == "steer-reject" {
            self.reject(command, STEERING_UNAVAILABLE);
            self.finish(true);
            return;
        }

        let steering = command["streamingBehavior"] == "steer";
        let key = if steering { "steering" } else { "followUp" };
        let mut queue = json!({
            "type": "queue_update", "steering": [], "followUp": [], "liveSteered": 0,
        });
        queue[key] = json!([command["message"]]);
        self.emit(&queue);
        self.response(command, Value::Null);
        self.queued_prompts.push(command["id"].clone());
        let fields = if steering {
            json!({"steering": true})
        } else {
            json!({})
        };
        self.message(
            "user",
            json!([{"type": "text", "text": command["message"]}]),
            fields,
        );
        self.finish(true);
    }

    fn steer(&mut self, command: &Value) {
        if self.current_prompt == "steer-reject" {
            self.reject(command, STEERING_UNAVAILABLE);
            self.finish(true);
            return;
        }
        self.response(command, Value::Null);
        if self.current_prompt == "wait-queue" {
            // OMP holds steering in its queue until the current message ends.
            self.queued_steers.push(command["message"].clone());
            self.emit(&json!({
                "type": "queue_update", "steering": self.queued_steers,
                "followUp": [], "liveSteered": 0,
            }));
            return;
        }
        self.message("user", command["message"].clone(), json!({}));
        self.finish(true);
    }

    async fn abort(&mut self, command: &Value) {
        match self.current_prompt.as_str() {
            "wait-slow-abort" => {
                fs::write(self.root.join("abort-entered"), []).expect("write abort marker");
                let release = self.root.join("release-abort");
                while !release.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            "wait-stuck-abort" => return,
            "wait-lost-abort" => {
                self.assistant("Stopped", "aborted");
                self.active = false;
                self.emit(&json!({
                    "type": "agent_end", "messages": [], "isTerminal": true, "yielded": true,
                }));
                self.prompt_result("aborted", true, false);
                return;
            }
            _ => {}
        }
        self.end_aborted_run();
        self.response(command, Value::Null);
        if self.queued_steers.is_empty() {
            self.prompt_result("aborted", true, true);
        } else {
            self.run_queued_steers();
        }
        self.emit(&json!({"type": "session_settled"}));
    }

    /// End an aborted run before the command response and prompt result.
    pub(super) fn end_aborted_run(&mut self) {
        self.message(
            "assistant",
            json!([{"type": "text", "text": "Stopped"}]),
            json!({
                "provider": "test", "model": "model", "stopReason": "aborted",
                "errorMessage": "Interrupted by user",
            }),
        );
        self.emit(&json!({"type": "agent_end", "messages": []}));
        self.active = false;
    }

    fn run_queued_steers(&mut self) {
        // OMP starts a new run for queued steering after aborting the previous one.
        self.emit(
            &json!({"type": "queue_update", "steering": [], "followUp": [], "liveSteered": 0}),
        );
        self.active = true;
        self.emit(&json!({"type": "agent_start"}));
        for steer in mem::take(&mut self.queued_steers) {
            self.message("user", steer, json!({}));
        }
        self.prompt_result("aborted", true, false);
        self.assistant("Steered reply", "stop");
        self.emit(&json!({"type": "agent_end", "messages": []}));
        self.active = false;
    }

    fn extension_ui_response(&mut self, command: &Value) {
        if self.pending_approval {
            let approved = matches!(
                command["value"].as_str(),
                Some("Approve" | "Approve bash for this session"),
            );
            let text = if approved {
                "ok\ndone"
            } else {
                "Tool call denied by user: bash"
            };
            self.tool_end("approval-call", "bash", text, !approved, Value::Null);
            self.pending_approval = false;
            self.finish(true);
        } else if self.ask_flow.is_some() {
            self.ask_answer(command);
        } else if let Some(pending) = self.pending_dialog.take() {
            self.complete_dialog(&pending);
        } else if let Some(pending) = self.pending_control.take() {
            self.response(&pending, json!({"value": command["value"]}));
        } else if let Some((pending, plugin)) = self.pending_plugin.take() {
            self.complete_plugin(&pending, &plugin, &command["value"]);
        } else {
            self.finish(true);
        }
    }

    fn complete_dialog(&mut self, pending: &Value) {
        self.response(pending, json!({"agentInvoked": true}));
        self.active = true;
        self.emit(&json!({"type": "agent_start"}));
        self.message(
            "user",
            json!([{"type": "text", "text": "interactive-fixture"}]),
            json!({}),
        );
        for queued in mem::take(&mut self.dialog_commands) {
            if queued["type"] == "steer" {
                self.response(&queued, json!({"disposition": "queued"}));
                self.message("user", queued["message"].clone(), json!({}));
            } else {
                self.response(
                    &queued,
                    json!({
                        "sessionId": self.session_id, "isSettled": false, "isStreaming": true,
                    }),
                );
            }
        }
        self.finish(true);
    }

    fn complete_plugin(&mut self, pending: &Value, plugin: &Value, value: &Value) {
        let name = plugin["name"].as_str().expect("fixture plugin name");
        let selected = value.as_str().unwrap_or("None");
        self.emit(&json!({
            "type": "extension_ui_request", "id": "plugin-note", "method": "notify",
            "message": format!("Plugin selected: {selected}"), "opaque": {"vendor": name},
        }));
        self.message(
            "custom",
            json!([{"type": "text", "text": format!("{name}: {selected}")}]),
            json!({"customType": name, "display": true}),
        );
        self.response(pending, json!({"agentInvoked": false}));
    }
}

fn rejects_subscription(command: &Value) -> bool {
    // An OMP without this subscription level refuses it.
    let refused = std::env::var("OMP_FIXTURE_REJECT_SUBSCRIPTION").unwrap_or_default();
    refused.split(',').any(|level| command["level"] == level)
}
