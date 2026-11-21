# Ait operation details

Read the live tool schema before calling an operation. These are orchestration rules and examples,
not a replacement for the current catalog.

## Profiles and next-turn settings

`profile_list` returns saved profiles, including notes. Apply a chosen profile's launch settings:

| Profile value                              | `agent_create` argument                                    |
| ------------------------------------------ | ---------------------------------------------------------- |
| `provider` and discovered or saved `model` | `provider` as `provider/model`                             |
| `modeId`                                   | `settings.modeId`                                          |
| `thinkingOptionId`                         | `settings.thinkingOptionId`                                |
| `featureValues`                            | `settings.features`                                        |
| `notes`                                    | Selection guidance; put the actual task in `initialPrompt` |

Omit absent optional settings. `agent_update` accepts metadata and runtime `settings`; its model
field is `settings.model`, whereas creation selects the model in the `provider/model` pair.
Use `agent_mode_set` for a mode-only change. Settings apply to the next turn through Provider rules;
they do not create an additional Agent.

## Workspace placement and isolation

An Agent-scoped creation inherits the caller's workspace and records parentage. An explicit
`workspaceId` places the child in that workspace while preserving the caller relationship.
Parentage does not itself provide a separate checkout.

Use `workspace_create` for isolation:

| Intent                          | Arguments in addition to source `path` or `projectId`                                              |
| ------------------------------- | -------------------------------------------------------------------------------------------------- |
| Use an existing local directory | `isolation: "local"`, with `path` or trusted caller cwd                                            |
| New branch from a base          | `isolation: "worktree"`, `mode: "branch-off"`; optional `baseBranch`, `branchName`, `worktreeSlug` |
| Existing branch                 | `isolation: "worktree"`, `mode: "checkout-branch"`, `branch`                                       |
| Pull/merge request checkout     | `isolation: "worktree"`, `mode: "checkout-pr"`, `prNumber`; optional `forge`                       |

Local isolation adopts an existing directory. Inspect `workspace_list` and the returned descriptor
for the actual workspace ID and path. Ait MCP does not expose background-workspace flags or an Agent
detach tool. Rename with `workspace_rename`; an external caller supplies `workspaceId` explicitly.

## Scripts and terminals

Use `script_list` to discover scripts configured in the workspace's `ait.json`.
`script_start` and `script_stop` require both `workspaceId` and the configured
`scriptName`. Use their supervised lifecycle rather than launching a second copy in a new terminal.

Each start records a run and returns `script.runId`. Pass `args` (an array of strings) only to
scripts whose listing shows `acceptsArgs: true`; each element reaches the script as one positional
parameter and is never interpreted by a shell. Call `script_run_wait` with the `runId`
(default 30 seconds, at most 45) and repeat while it returns `timedOut: true`; a timeout does not
stop the run. The finished run reports `outcome`, `exitCode`, `durationMs`, the `commit` and
`dirty` state it started from, and bounded combined output. `script_run_get` reads a run
by `runId`, or the latest run of a `scriptName`, including output while it is still running.
Treat a `succeeded` run of the workspace's verification script at a clean `commit` as evidence for
that commit only.

For an interactive process, `terminal_create` accepts `cwd` or `workspaceId`; a trusted caller can
supply inherited placement. Use the returned terminal ID for `terminal_capture`,
`terminal_send`, and `terminal_kill`.

Use `literal: true` for text and command content. Send a separate `keys: "Enter"` when a newline is
needed. Supported key tokens include `C-c`, `C-d`, `Tab`, and `Escape`. Terminal input can affect a
running program immediately; inspect the capture when its current state is unclear. Capture ranges
are inclusive, support negative offsets, and contain rendered plain text.

## Schedules and heartbeats

Use `schedule_create` for cron work that launches a new Agent per run in a fixed workspace.
An external caller supplies `workspaceId` and `provider`; a trusted Agent caller can inherit them.
Resolve the user's timezone to an explicit IANA timezone when wall-clock timing matters; omission
uses UTC. Verify cron, lifetime limits, and any requested initial run before creating it.

Typical argument shapes, with values selected from the user's task and live discovery:

```json
{
  "name": "schedule_create",
  "arguments": {
    "workspaceId": "wks_example",
    "provider": "provider/model",
    "prompt": "Run the requested checks and summarize their results.",
    "cron": "0 9 * * 1-5",
    "timezone": "Asia/Shanghai",
    "maxRuns": 5,
    "runOnCreate": false
  }
}
```

Replace example IDs and provider/model values with actual results. Use `schedule_get` and
`schedule_log_read` for status and runs. `schedule_run` adds a run without changing cron;
pausing and resuming use their dedicated tools. `schedule_update` takes `scheduleId`: `cron` and `every`
are alternatives, and `expiresIn` and `clearExpires: true` are alternatives.

Use `heartbeat_create` only to send recurring prompts into the current, trusted calling Agent.
Ait heartbeats persist across daemon restarts and need explicit `heartbeat_delete` cleanup.
`schedule_list` and the ordinary schedule mutation tools operate on new-Agent schedules, not
heartbeats; `heartbeat_list` returns the calling Agent's heartbeats with the `heartbeatId` each
needs. `heartbeat_delete` requires the original caller's identity.
