//! Recorded and synthetic prompt scenarios used by the offline OMP tests.

use std::io::{self, Write};

use serde_json::{Value, json};

use super::{Peer, kill_self, read_json, write_stdout};

const UPSTREAM_ERROR: &str = "Upstream shard unavailable api_key=fixture-secret";
const BALANCE_ERROR: &str = "Payment required: insufficient balance api_key=fixture-secret";

enum PromptAction {
    Complete,
    Wait,
    Exit,
}

impl Peer {
    /// Answer the prompt and run its scenario, returning whether stdin should remain open.
    pub(super) fn prompt(&mut self, command: &Value) -> bool {
        let text = command["message"].as_str().expect("prompt text");
        text.clone_into(&mut self.current_prompt);
        self.prompt_id = command["id"].clone();
        if self.handle_local_prompt(command, text) {
            return true;
        }

        self.response(command, json!({"agentInvoked": true}));
        self.active = true;
        self.emit(&json!({"type": "agent_start"}));
        if text.starts_with("/skill:") {
            self.skill_prompt(text);
            // A steer completes the turn when the skill is waiting.
            if !text.ends_with(" wait") {
                self.finish(true);
            }
            return true;
        }

        self.prompt_message(command, text);
        match self.run_prompt_scenario(text) {
            PromptAction::Complete => {
                let call = self.inspect_file();
                if matches!(text, "background-tool-error" | "background-busy") {
                    self.background_tool_error(&call, text == "background-busy");
                }
                self.finish(text != "background");
                true
            }
            PromptAction::Wait => true,
            PromptAction::Exit => false,
        }
    }

    fn handle_local_prompt(&mut self, command: &Value, text: &str) -> bool {
        if self.start_plugin(command, text) {
            return true;
        }
        match text {
            "interactive-fixture" => {
                self.pending_dialog = Some(command.clone());
                let dialog = read_json(&self.root.join("dialog.json"));
                let mut frame = json!({"type": "extension_ui_request", "id": "dialog-1"});
                frame
                    .as_object_mut()
                    .expect("dialog frame")
                    .extend(dialog.as_object().expect("dialog fixture").clone());
                self.emit(&frame);
            }
            "reject" => self.reject(command, "Rejected"),
            "/notify-only" => {
                self.emit(&json!({
                    "type": "extension_ui_request",
                    "method": "notify",
                    "message": "Notify only output",
                    "notifyType": "info"
                }));
                self.response(command, Value::Null);
                self.prompt_result("completed", false, true);
            }
            "/local" | "/local-result" => {
                // OMP 18.18.8 writes a builtin command's output before acknowledging it.
                self.emit(&json!({
                    "type": "command_output",
                    "text": "\u{1b}[1mSession\u{1b}[0m: \u{1b}]8;;file:///x\u{7}local\u{1b}]8;;\u{7} \n"
                }));
                self.response(command, json!({"agentInvoked": false}));
                if text == "/local-result" {
                    // Older OMP releases also report a prompt result for builtins.
                    self.prompt_result("completed", false, true);
                }
            }
            "late-reject" | "late-reject-result" => {
                self.response(command, Value::Null);
                self.emit(&json!({
                    "type": "response", "id": self.prompt_id, "command": "prompt",
                    "success": false, "error": "No API key found for test."
                }));
                if text == "late-reject-result" {
                    self.emit(&json!({
                        "type": "prompt_result",
                        "id": self.prompt_id,
                        "agentInvoked": false,
                        "status": "error",
                        "sessionSettled": true,
                        "error": {"message": "No API key found for test.", "retryable": false}
                    }));
                }
            }
            _ => return false,
        }
        true
    }

    fn start_plugin(&mut self, command: &Value, text: &str) -> bool {
        let Some(name) = text.strip_prefix('/') else {
            return false;
        };
        let Some(plugin) = self
            .read_plugins()
            .into_iter()
            .find(|plugin| plugin["name"].as_str() == Some(name))
        else {
            return false;
        };
        self.emit(&json!({
            "type": "extension_ui_request",
            "id": "plugin-question",
            "method": "select",
            "title": plugin["description"],
            "options": ["A, B", "C"]
        }));
        self.pending_plugin = Some((command.clone(), plugin));
        true
    }

    fn prompt_message(&mut self, command: &Value, text: &str) {
        let images = command["images"].as_array().map_or(&[][..], Vec::as_slice);
        let supports_images = self.model["input"]
            .as_array()
            .is_some_and(|inputs| inputs.iter().any(|input| input == "image"));
        if !images.is_empty() && !supports_images {
            // OMP retains the images and records a hidden user-attributed description.
            let descriptions: Vec<_> = images
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    json!({
                        "type": "text",
                        "text": format!(
                            "<image path=\"local://image-{index}.webp\">\n\
                             [No vision-capable model is configured, so this image could not be \
                             described automatically.]\n</image>"
                        )
                    })
                })
                .collect();
            self.custom_message(
                "image-attachment-description",
                Value::Array(descriptions),
                Value::Null,
                "user",
                false,
            );
        }
        let mut content = Vec::with_capacity(images.len() + 1);
        content.push(json!({"type": "text", "text": text}));
        content.extend(images.iter().cloned());
        let (message, entry_id) = self.message("user", Value::Array(content), Value::Null);
        if text == "duplicate-user" {
            // Exercise deduplication after the same user message is announced again.
            self.announce(&message, &entry_id, "start");
            self.announce(&message, &entry_id, "end");
        }
    }

    fn run_prompt_scenario(&mut self, text: &str) -> PromptAction {
        match text {
            "wait-config" => return PromptAction::Wait,
            "closed" => return PromptAction::Exit,
            "crash" => crash(),
            "killed" => kill_self(),
            "incomplete-frame" => {
                write_stdout("{\"type\":");
                return PromptAction::Exit;
            }
            "invalid-chunks" => self.invalid_chunks(),
            "non-object-line" => self.non_object_lines(),
            "garbage-line" => {
                for line in ["{broken api_key=fixture-secret", "", "   "] {
                    self.write_line(line);
                }
            }
            "native-error" | "native-error-recovered" => {
                if self.native_error(text == "native-error") {
                    return PromptAction::Wait;
                }
            }
            "subagents" => self.subagent_task(),
            "todo-state" => {
                self.todo_phases = json!([{
                    "name": "Plan", "tasks": [{"content": "Check state", "status": "pending"}]
                }]);
            }
            "chunked" => self.emit(&json!({
                "type": "available_commands_update",
                "commands": [{"name": "large", "description": "x".repeat(1_200_000)}]
            })),
            "timeline-mapping" => self.timeline_mapping(),
            "unknown-tool-result" => {
                self.message(
                    "toolResult",
                    json!([{"type": "text", "text": "unmatched result"}]),
                    json!({"toolCallId": "unknown", "toolName": "bash", "isError": true}),
                );
            }
            "uncorrelated-failure" => self.emit(&json!({
                "type": "response", "id": "not-a-request", "command": "get_state",
                "success": false, "error": "Unknown request"
            })),
            _ if self.hold_prompt(text) => return PromptAction::Wait,
            _ => {}
        }
        PromptAction::Complete
    }

    fn hold_prompt(&mut self, text: &str) -> bool {
        match text {
            "native-host" => {
                self.emit(
                    &json!({"type": "future_native_event", "opaque": {"unmodified": [1, 2, 3]}}),
                );
                self.emit(&json!({
                    "type": "host_tool_call", "id": "host-task", "toolCallId": "external-tool",
                    "toolName": "unknown-vendor-tool", "arguments": {"value": 42}
                }));
            }
            "frame-error" => self.emit(&json!({
                "type": "rpc_frame_error", "error": {"message": UPSTREAM_ERROR, "statusCode": 503}
            })),
            "unknown-native-error" => {
                self.message(
                    "assistant",
                    json!([]),
                    json!({"stopReason": "error", "errorMessage": UPSTREAM_ERROR}),
                );
                self.failed_prompt(UPSTREAM_ERROR);
            }
            "invalid-timestamp" => self.emit(&json!({
                "type": "message_start", "message": {"role": "assistant", "content": []}
            })),
            "invalid-usage" => {
                self.message(
                    "assistant",
                    json!([]),
                    json!({"usage": {"input": -1, "output": 0}}),
                );
            }
            "lost-settlement" | "lost-result" | "busy-after-yield" | "missing-settlement-fact" => {
                self.yield_unsettled(text);
            }
            "wait" | "wait-slow-abort" | "wait-lost-abort" | "wait-stuck-abort" | "wait-queue"
            | "steer-reject" => {
                self.emit(&json!({"type": "agent_end", "messages": []}));
                self.emit(&json!({"type": "auto_retry_start", "attempt": 1}));
            }
            "tool-abort" => self.stuck_tool(),
            "approval" => self.request_approval(),
            "open-url" => {
                self.ui_request(
                    "open_url",
                    json!({
                        "url": "https://example.com/device",
                        "launchUrl": "https://example.com/device?code=ABCD",
                        "instructions": "Enter code ABCD"
                    }),
                );
                self.finish(true);
            }
            "html-ask" => self.html_question(),
            "ask" => self.emit(&json!({
                "type": "extension_ui_request", "id": "ask-1", "method": "ask",
                "questions": [
                    {"id": "db", "question": "Which database?",
                     "options": [{"label": "A,B"}, {"label": "C"}]},
                    {"id": "features", "question": "Which features?", "multi": true,
                     "options": [{"label": "Auth"}, {"label": "Search"}]}
                ]
            })),
            "question" => self.emit(&json!({
                "type": "extension_ui_request", "id": "ui-1", "method": "select",
                "title": "Choose", "options": ["A", "B"]
            })),
            _ => return false,
        }
        true
    }

    fn native_error(&mut self, terminal: bool) -> bool {
        self.message(
            "assistant",
            json!([{"type": "text", "text": format!("Partial {}", "x".repeat(600))}]),
            json!({
                "provider": "test", "model": "model", "stopReason": "error",
                "responseModel": "test-model-2026", "responseId": "resp_1",
                "errorMessage": BALANCE_ERROR
            }),
        );
        if terminal {
            self.active = false;
            self.failed_prompt(BALANCE_ERROR);
        }
        terminal
    }

    fn failed_prompt(&self, error: &str) {
        self.emit(&json!({
            "type": "prompt_result", "id": self.prompt_id, "status": "error",
            "sessionSettled": true, "error": error
        }));
    }

    fn non_object_lines(&self) {
        for line in ["[1,2]", "\"text\"", "42", "null"] {
            self.write_line(line);
        }
        self.emit(&json!({"content": "api_key=fixture-secret"}));
        self.emit(&json!({"type": 7}));
    }

    fn invalid_chunks(&self) {
        const LARGE: u64 = 1_048_576;
        let long_id = "x".repeat(129);
        for (id, index, count, length, data) in [
            ("stray", 1, 5, LARGE, "e30="),
            ("single", 0, 1, LARGE, "e30="),
            ("small", 0, 2, 4, "e30="),
            ("padding", 0, 5, LARGE, "e31="),
            (long_id.as_str(), 0, 5, LARGE, "e30="),
            ("short", 0, 2, LARGE, "e30="),
            ("short", 1, 2, LARGE, "e30="),
        ] {
            self.emit(&json!({
                "type": "rpc_chunk", "chunkId": id, "index": index, "count": count,
                "byteLength": length, "data": data
            }));
        }
    }

    fn yield_unsettled(&mut self, text: &str) {
        self.assistant("Hello world", "stop");
        self.emit(
            &json!({"type": "agent_end", "messages": [], "isTerminal": true, "yielded": true}),
        );
        self.active = false;
        self.pending_async = text == "busy-after-yield";
        if text != "lost-result" {
            self.prompt_result("completed", true, false);
        }
    }

    fn stuck_tool(&mut self) {
        self.tool_call("stuck-tool", "bash", json!({"command": "sleep 30"}));
        self.emit(&json!({
            "type": "tool_execution_start", "toolCallId": "stuck-tool", "toolName": "bash",
            "args": {"command": "sleep 30"}
        }));
        self.emit(&json!({
            "type": "subagent_lifecycle",
            "payload": {"id": "worker", "index": 0, "agent": "Task", "status": "started",
                        "parentToolCallId": "stuck-tool"}
        }));
    }

    fn request_approval(&mut self) {
        let command = "printf 'ok'\necho done";
        self.tool_call("approval-call", "bash", json!({"command": command}));
        self.emit(&json!({
            "type": "tool_execution_start", "toolCallId": "approval-call", "toolName": "bash",
            "args": {"command": command}
        }));
        self.pending_approval = true;
        self.ui_request(
            "select",
            json!({
                "title": format!("Allow tool: bash\nCommand: {command}"),
                "options": ["Approve", "Approve bash for this session", "Deny"]
            }),
        );
    }

    fn html_question(&mut self) {
        let html = "<form>…</form>";
        self.tool_call(
            "ask-call",
            "ask",
            json!({"i": "Pick a stack", "html": html}),
        );
        // OMP asks the first field before announcing the tool's execution.
        self.start_ask();
        self.emit(&json!({
            "type": "tool_execution_start", "toolCallId": "ask-call", "toolName": "ask",
            "args": {"html": html}, "intent": "Pick a stack"
        }));
    }

    fn inspect_file(&mut self) -> String {
        let call = format!("call-{}", self.clock);
        self.message(
            "assistant",
            json!([
                {"type": "thinking", "thinking": "Plan"},
                {"type": "text", "text": "Inspect"},
                {"type": "toolCall", "id": call, "name": "read", "arguments": {"path": "file.rs"}}
            ]),
            json!({
                "provider": "test", "model": "model", "stopReason": "toolUse",
                "usage": {"input": 10, "output": 5, "cacheRead": 2, "cacheWrite": 0,
                          "cost": {"total": 0.001}}
            }),
        );
        self.emit(&json!({
            "type": "tool_execution_start", "toolCallId": call, "toolName": "read",
            "args": {"path": "file.rs"}
        }));
        self.tool_end(
            &call,
            "read",
            "contents",
            false,
            json!({
                "totalLines": 1, "rowPrefixes": true,
                "displayContent": {"text": "contents", "startLine": 1, "lineNumbers": [1]}
            }),
        );
        call
    }

    fn background_tool_error(&mut self, call: &str, busy: bool) {
        if busy {
            self.message(
                "assistant",
                json!([{
                    "type": "toolCall", "id": "foreground", "name": "bash",
                    "arguments": {"command": "offline fixture"}
                }]),
                json!({"stopReason": "toolUse"}),
            );
            self.emit(&json!({
                "type": "tool_execution_start", "toolCallId": "foreground", "toolName": "bash",
                "args": {"command": "offline fixture"}
            }));
        } else {
            self.emit(&json!({"type": "agent_end", "messages": []}));
            self.emit(&json!({
                "type": "prompt_result", "id": self.prompt_id,
                "status": "completed", "sessionSettled": false
            }));
        }

        self.emit(&json!({
            "type": "tool_execution_update", "toolCallId": call, "toolName": "read",
            "args": {"path": "file.rs"},
            "partialResult": {
                "content": [{"type": "text", "text": "curl: (28) Connection timed out"}],
                "details": {"async": {"state": "completed", "jobId": "bg_15"}}
            }
        }));
        self.custom_message(
            "async-result",
            json!(
                "<system-notice>\nBackground job bg_15 (curl) has completed. Resume your work \
                 using the result below.\ncurl: (28) Connection timed out\n</system-notice>"
            ),
            json!({
                "jobs": [{"jobId": "bg_15", "type": "bash", "status": "completed", "label": "curl"}]
            }),
            "agent",
            true,
        );
        if busy {
            self.message(
                "toolResult",
                json!([{"type": "text", "text": "Foreground finished"}]),
                json!({"toolCallId": "foreground", "toolName": "bash", "isError": false}),
            );
        } else {
            self.emit(&json!({"type": "agent_start"}));
        }
    }
}

fn crash() -> ! {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all("filler diagnostic line\n".repeat(400).as_bytes());
    let _ = writeln!(
        stderr,
        "fatal: worker crashed api_key=fixture-secret see \
         https://example.com/private/path?token=fixture-secret"
    );
    let _ = stderr.flush();
    std::process::exit(3);
}
