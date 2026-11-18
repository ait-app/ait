use serde_json::{Value, json};

use super::{Peer, id, write_records};

struct TimelineTool {
    call: &'static str,
    name: &'static str,
    arguments: Value,
    text: &'static str,
    details: Value,
}

impl Peer {
    /// Expand an OMP `/skill:` command into a user-attributed custom message.
    pub(super) fn skill_prompt(&mut self, text: &str) {
        let invocation = text
            .strip_prefix("/skill:")
            .expect("skill invocation prefix");
        let (name, arguments) = invocation.split_once(' ').unwrap_or((invocation, ""));
        let details = json!({
            "name": name,
            "path": format!("{}/.omp/skills/{name}/SKILL.md", self.cwd.display()),
            "args": arguments,
            "prompt": text,
            "lineCount": 1,
        });
        let content = format!(
            "[IMPORTANT: User invoked the \"{name}\" skill; follow its instructions. \
             Full skill below.]\n\nReview the code carefully.\nUser: {arguments}"
        );
        self.custom_message("skill-prompt", json!(content), details, "user", true);
    }

    /// Record an assistant tool call with its native call ID, tool name, and arguments.
    pub(super) fn tool_call(&mut self, call: &str, name: &str, arguments: Value) {
        self.tool_calls(Value::Array(vec![tool_call_block(call, name, arguments)]));
    }

    /// Emit native tool completion and record the same result in the branch.
    /// `details` can be null when the named call has no native result metadata.
    pub(super) fn tool_end(
        &mut self,
        call: &str,
        name: &str,
        text: &str,
        error: bool,
        details: Value,
    ) {
        let content = json!([{"type": "text", "text": text}]);
        let details = if details.is_null() {
            json!({})
        } else {
            details
        };
        self.emit(&json!({
            "type": "tool_execution_end", "toolCallId": call, "toolName": name,
            "result": {"content": content, "details": details}, "isError": error,
        }));
        self.message(
            "toolResult",
            content,
            json!({"toolCallId": call, "toolName": name, "details": details, "isError": error}),
        );
    }

    /// Exercise native timeline mapping with tool output and asynchronous notices.
    pub(super) fn timeline_mapping(&mut self) {
        // OMP 18.18.8 supplies these tool shapes. The search failure also follows
        // Paseo's handling of `details.error` when `isError` is false.
        let tools = [
            TimelineTool {
                call: "edit-1",
                name: "edit",
                arguments: json!({"i": "e", "input": "[probe.txt#0097]\nPUT 1.=1:\n+line 1\n"}),
                text: "[probe.txt#5D8B]\n1:line 1\n2:line two",
                details: json!({
                    "diff": "-1|line one\n+1|line 1\n 2|line two", "firstChangedLine": 1,
                    "op": "update", "path": self.cwd.join("probe.txt").display().to_string(),
                    "oldText": "line one\nline two\n", "newText": "line 1\nline two\n",
                }),
            },
            TimelineTool {
                call: "read-1",
                name: "read",
                arguments: json!({"i": "r", "path": "probe.txt"}),
                text: "[probe.txt#5D8B]\n1:line 1",
                details: json!({
                    "totalLines": 1, "rowPrefixes": true,
                    "displayContent": {"text": "line 1", "startLine": 1, "lineNumbers": [1]},
                }),
            },
            TimelineTool {
                call: "search-1",
                name: "web_search",
                arguments: json!({"i": "s", "query": "rust serde"}),
                text: "",
                details: json!({"error": "All web search providers failed"}),
            },
            TimelineTool {
                call: "todo-1",
                name: "todo",
                arguments: json!({
                    "i": "t", "op": "init", "list": [{"phase": "Build", "items": ["one", "two"]}],
                }),
                text: "Remaining items (2)",
                details: json!({
                    "op": "init", "storage": "session", "phases": [{"name": "Build", "tasks": [
                        {"content": "one", "status": "in_progress"},
                        {"content": "two", "status": "blocked", "blocker": "waiting on api"},
                        {"content": "three", "status": "abandoned"},
                    ]}],
                }),
            },
            TimelineTool {
                call: "task-1",
                name: "task",
                arguments: json!({
                    "i": "delegate", "context": "# Goal", "tasks": [
                        {"task": "# Target\nReply", "solutionSpace": "one fix", "name": "Prober"},
                    ],
                }),
                text: "Spawned agent `Prober` (job `Prober`).",
                details: json!({
                    "results": [], "async": {"state": "running", "jobId": "Prober", "type": "task"},
                }),
            },
        ];
        self.emit_timeline_tools(tools);
        self.timeline_notices();
    }

    fn tool_calls(&mut self, blocks: Value) {
        self.message(
            "assistant",
            blocks,
            json!({"provider": "test", "model": "model", "stopReason": "toolUse"}),
        );
    }

    fn emit_timeline_tools(&mut self, mut tools: [TimelineTool; 5]) {
        self.tool_calls(Value::Array(
            tools
                .iter()
                .map(|tool| tool_call_block(tool.call, tool.name, tool.arguments.clone()))
                .collect(),
        ));
        for tool in &mut tools {
            let arguments = tool
                .arguments
                .as_object_mut()
                .expect("fixture tool arguments");
            let intent = arguments.remove("i").expect("fixture tool intent");
            self.emit(&json!({
                "type": "tool_execution_start", "toolCallId": tool.call, "toolName": tool.name,
                "args": arguments, "intent": intent,
            }));
        }
        for tool in tools {
            self.tool_end(tool.call, tool.name, tool.text, false, tool.details);
        }
    }

    fn timeline_notices(&mut self) {
        // Compaction, goal, and retry frames follow the OMP RPC serializer and Paseo.
        for frame in [
            json!({
                "type": "notice", "level": "warning", "source": "cost-gate",
                "message": "Cost warning: this session has spent $1.00, \
                            past its $0.50 warning threshold.",
            }),
            json!({
                "type": "goal_updated",
                "goal": {"id": "goal-1", "objective": "Ship", "status": "active"},
                "state": {"enabled": true, "mode": "goal"},
            }),
            json!({
                "type": "auto_retry_start", "attempt": 1, "maxAttempts": 3, "delayMs": 2000,
                "errorMessage": "overloaded", "errorId": 529,
            }),
            json!({"type": "auto_retry_end", "success": true, "attempt": 1}),
            json!({
                "type": "retry_fallback_applied", "from": "test/model", "to": "other/model",
                "role": "default", "reason": "rate limit",
            }),
            json!({"type": "retry_fallback_succeeded", "model": "other/model", "role": "default"}),
            json!({"type": "auto_compaction_start", "reason": "threshold", "action": "handoff"}),
            json!({
                "type": "auto_compaction_end", "action": "handoff", "aborted": false,
                "willRetry": false, "result": {"summary": "Summary", "tokensBefore": 100_010},
            }),
            json!({"type": "manual_compaction_start"}),
            json!({
                "type": "manual_compaction_end", "aborted": false, "skipped": true,
                "errorMessage": "Nothing to compact (session too small)",
            }),
            json!({"type": "compaction_start", "reason": "manual"}),
            json!({"type": "compaction_end", "reason": "manual"}),
        ] {
            self.emit(&frame);
        }
        self.timeline_custom_messages();
        self.emit(&json!({
            "type": "todo_reminder", "todos": [{"content": "one", "status": "in_progress"}],
            "attempt": 1, "maxAttempts": 3,
        }));
    }

    fn timeline_custom_messages(&mut self) {
        self.custom_message(
            "async-result",
            json!(
                "<system-notice>\nBackground job Prober has completed. Resume your work using \
                 the result below.\n<task-result id=\"Prober\" agent=\"task\" status=\"completed\" \
                 duration=\"50ms\">\n\n<output>\nchild done\n</output>\n</task-result>\n\n\
                 Prober is now idle\n</system-notice>"
            ),
            json!({"jobs": [{
                "jobId": "Prober", "type": "task", "status": "completed",
                "agentId": "Prober", "label": "Prober", "durationMs": 49,
            }]}),
            "agent",
            true,
        );
        self.custom_message(
            "advisor",
            json!("Advice"),
            json!({"notes": [{
                "note": "Handle the error", "severity": "blocker", "advisor": "rust",
            }]}),
            "agent",
            true,
        );
        self.custom_message(
            "vendor-note",
            json!("Plain extension output"),
            json!({"k": 1}),
            "agent",
            true,
        );
        self.custom_message(
            "hidden-context",
            json!("Private context"),
            Value::Null,
            "agent",
            false,
        );
    }

    /// Simulate a delegated OMP task and persist its native child session.
    pub(super) fn subagent_task(&mut self) {
        // OMP 18.18.8 writes the child beside the parent's history and supplies its path
        // in lifecycle and progress frames. A synchronous result follows Paseo's reader.
        let child_file = self.file.with_extension("").join("worker.jsonl");
        let session_file = child_file.display().to_string();
        let tasks = json!([{"task": "# Target\nReview", "name": "worker"}]);
        self.tool_call(
            "task-1",
            "task",
            json!({"i": "Delegate review", "context": "# Goal", "tasks": tasks}),
        );
        self.emit(&json!({
            "type": "tool_execution_start", "toolCallId": "task-1", "toolName": "task",
            "args": {"context": "# Goal", "tasks": tasks}, "intent": "Delegate review",
        }));
        let mut lifecycle = json!({
            "id": "worker", "index": 0, "agent": "Task", "agentSource": "bundled",
            "parentToolCallId": "task-1", "sessionFile": session_file,
            "description": "Review the change", "status": "started",
        });
        self.emit(&json!({"type": "subagent_lifecycle", "payload": lifecycle}));
        let message = self.emit_subagent_progress(&session_file);
        write_records(&child_file, &self.child_session_records(&message));
        lifecycle["status"] = json!("completed");
        self.emit(&json!({"type": "subagent_lifecycle", "payload": lifecycle}));
        self.tool_end(
            "task-1",
            "task",
            "worker finished",
            false,
            json!({
                "results": [{"id": "worker", "agent": "Task", "exitCode": 0}],
                "progress": [{"index": 0, "id": "worker", "agent": "Task", "status": "completed",
                              "assignment": "# Target\nReview"}],
            }),
        );
    }

    fn emit_subagent_progress(&self, session_file: &str) -> Value {
        self.emit(&json!({"type": "subagent_progress", "payload": {
            "index": 0, "agent": "Task", "agentSource": "bundled", "task": "Complete assignment",
            "assignment": "# Target\nReview", "parentToolCallId": "task-1",
            "sessionFile": session_file, "progress": {
                "index": 0, "id": "worker", "agent": "Task", "status": "running",
                "task": "Complete assignment", "assignment": "# Target\nReview",
                "resolvedModel": "test/model",
            },
        }}));
        let message = json!({
            "role": "assistant", "timestamp": 1_792_540_800_300_u64, "provider": "test",
            "model": "model", "stopReason": "stop",
            "content": [{"type": "text", "text": "Child output"}],
        });
        self.emit(
            &json!({"type": "subagent_event", "payload": {"id": "worker", "event": {
                "type": "message_end", "message": message,
            }}}),
        );
        message
    }

    fn child_session_records(&self, message: &Value) -> [Value; 6] {
        const CREATED: &str = "2026-10-21T00:00:02.000Z";
        [
            json!({"type": "title", "v": 1, "title": "", "updatedAt": CREATED}),
            json!({
                "type": "session", "version": 3, "id": id(), "cwd": self.cwd.display().to_string(),
                "timestamp": CREATED, "parentSession": self.file.display().to_string(),
            }),
            json!({
                "type": "model_change", "id": "c1", "parentId": null, "timestamp": CREATED,
                "model": "test/model",
            }),
            json!({
                "type": "session_init", "id": "c2", "parentId": "c1", "timestamp": CREATED,
                "systemPrompt": ["child system prompt"],
            }),
            json!({
                "type": "message", "id": "c3", "parentId": "c2",
                "timestamp": "2026-10-21T00:00:02.100Z",
                "message": {"role": "user", "content": [{"type": "text", "text": "Review"}],
                            "attribution": "agent", "timestamp": 1_792_540_800_200_u64},
            }),
            json!({
                "type": "message", "id": "c4", "parentId": "c3",
                "timestamp": "2026-10-21T00:00:02.300Z",
                "message": message,
            }),
        ]
    }
}

fn tool_call_block(call: &str, name: &str, arguments: Value) -> Value {
    let mut block = json!({"type": "toolCall", "id": call, "name": name});
    block["arguments"] = arguments;
    block
}
