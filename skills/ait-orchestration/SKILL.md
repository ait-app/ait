---
name: ait-orchestration
description: "Orchestrate coding agents through Ait daemon. Use when the user asks to delegate work through Ait, coordinate agents in workspaces, manage Ait scripts or terminals, or create Ait schedules and heartbeats. Ordinary coding tasks do not imply a request to delegate."
---

# Ait Orchestration

Use Ait to coordinate work in the user's chosen host and workspace. Keep the requested task,
provider/profile choices, and resource lifetime explicit.

## Connect and discover

Use the connected Ait MCP tools when available. Tool prefixes vary by client; identify the server
and tool names from the current environment. For multiple hosts, use the host the user selected.

If MCP tools are unavailable but shell execution is available, read
[MCP access](references/mcp-access.md) for the HTTP fallback. That path requires connection details
supplied by the runtime or user. If neither path is available, report the missing connection;
the skill does not install a server, enable injection, or obtain credentials by itself.

Treat the live `tools/list` input schemas as authoritative. Use only tools present in that catalog, and use
their current arguments rather than reconstructing retired Paseo or WebSocket request shapes.
Inspect `result.isError` for MCP execution failures as well as JSON-RPC and transport errors.

The calling Agent identity must come from the trusted runtime or user. A workspace ID is a location;
an Agent ID is a session. Use each returned ID in its own role. Without a supplied caller identity,
operate as an external caller and provide explicit placement where needed.

## Delegate a task

1. Read `list_profiles` and use profile notes to choose the user's requested profile, or an appropriate
   saved profile when none was named. Profiles guide launch settings; `create_agent` has no profile
   argument. Use `list_providers` / `list_models` for missing provider/model choices, and
   `inspect_provider` when mode, thinking, or feature settings need validation.
2. Reuse the requested workspace. For separate checkouts, create a workspace with
   `create_workspace`, then pass its returned ID to `create_agent`. An external caller supplies
   `workspaceId` or an absolute `cwd`; an Agent-scoped call can inherit its caller's workspace.
3. Call `create_agent` with a discovered `provider/model`, a short title, and `initialPrompt`.
   State the concrete task, allowed scope, shared-file ownership when workers run concurrently,
   expected deliverable, and relevant validation. Request further delegation only when the user's
   assignment includes it. Read [operation details](references/operations.md) for profile mapping
   and workspace isolation choices.
4. Creation and `send_agent_prompt` acknowledge admission, not completion. Continue independent
   work where useful. Use `wait_for_agent` for bounded waiting and `get_agent_status` /
   `get_agent_activity` to inspect progress and output. Follow `nextCursor` when listing additional
   agents or workspaces is necessary.
5. Verify the requested result before reporting completion. Include the relevant Agent/Workspace
   IDs, outcome, validation, and remaining work. Preserve ongoing resources; clean up resources
   designated temporary by the user's task.

Use a stable `idempotencyKey` for each logical Agent/Workspace creation and reuse it with the same
arguments after an uncertain transport result. Use a stable `messageId` when retrying one prompt.
A JSON-RPC request ID is only a response correlation ID. Inspect the prior operation before retrying
other mutations after a lost response; blindly repeating a run or terminal input can duplicate work.

## Wait, intervene, and finish

`wait_for_agent` defaults to 30 seconds and accepts at most 45 seconds. A `timeout` leaves the
Agent running. Check status and continue waiting as the task requires; do not restart the Agent
or declare failure solely because a wait timed out.

When work is blocked on permission, inspect `list_pending_permissions` or the Agent snapshot.
Use `respond_to_permission` only for a decision the user authorized, with that Agent's request ID
and advertised response payload. Surface decisions needing user input; approvals are not a way
to make an otherwise blocked task finish automatically. Bot permission requests follow daemon rules.

`cancel_agent` interrupts the current turn while retaining the Agent. `archive_agent` closes and
archives it; `kill_agent` also closes and archives while retaining history. `archive_workspace`
retires its Agents, scripts, and terminals and can remove an owned worktree. Apply lifecycle actions
to the resources and scope requested by the user.

## Scripts, terminals, and recurring work

Read [operation details](references/operations.md) when starting/stopping configured workspace
scripts, sending terminal input, changing runtime settings, or managing schedules and heartbeats.
Recurring work requires a request for recurring execution; a long task alone is not a heartbeat.
Report schedule/heartbeat IDs and retain them for inspection or deletion.
