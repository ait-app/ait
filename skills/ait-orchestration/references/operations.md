# Ait operation details

Read the live tool schema before calling an operation. These are orchestration rules and examples,
not a replacement for the current catalog.

## Profiles and next-turn settings

`list_profiles` returns saved profiles, including notes. Apply a chosen profile's launch settings:

| Profile value                              | `create_agent` argument                                    |
| ------------------------------------------ | ---------------------------------------------------------- |
| `provider` and discovered or saved `model` | `provider` as `provider/model`                             |
| `modeId`                                   | `settings.modeId`                                          |
| `thinkingOptionId`                         | `settings.thinkingOptionId`                                |
| `featureValues`                            | `settings.features`                                        |
| `notes`                                    | Selection guidance; put the actual task in `initialPrompt` |

Omit absent optional settings. `update_agent` accepts metadata and runtime `settings`; its model
field is `settings.model`, whereas creation selects the model in the `provider/model` pair.
Use `set_agent_mode` for a mode-only change. Settings apply to the next turn through Provider rules;
they do not create an additional Agent.

## Workspace placement and isolation

An Agent-scoped creation inherits the caller's workspace and records parentage. An explicit
`workspaceId` places the child in that workspace while preserving the caller relationship.
Parentage does not itself provide a separate checkout.

Use `create_workspace` for isolation:

| Intent                          | Arguments in addition to source `path` or `projectId`                                              |
| ------------------------------- | -------------------------------------------------------------------------------------------------- |
| Use an existing local directory | `isolation: "local"`, with `path` or trusted caller cwd                                            |
| New branch from a base          | `isolation: "worktree"`, `mode: "branch-off"`; optional `baseBranch`, `branchName`, `worktreeSlug` |
| Existing branch                 | `isolation: "worktree"`, `mode: "checkout-branch"`, `branch`                                       |
| Pull/merge request checkout     | `isolation: "worktree"`, `mode: "checkout-pr"`, `prNumber`; optional `forge`                       |

Local isolation adopts an existing directory. Inspect `list_workspaces` and the returned descriptor
for the actual workspace ID and path. Ait MCP does not expose background-workspace flags or an Agent
detach tool. Rename with `rename_workspace`; an external caller supplies `workspaceId` explicitly.

## Scripts and terminals

Use `list_workspace_scripts` to discover scripts configured in the workspace's `ait.json`.
`start_workspace_script` and `stop_workspace_script` require both `workspaceId` and the configured
`scriptName`. Use their supervised lifecycle rather than launching a second copy in a new terminal.

For an interactive process, `create_terminal` accepts `cwd` or `workspaceId`; a trusted caller can
supply inherited placement. Use the returned terminal ID for `capture_terminal`,
`send_terminal_keys`, and `kill_terminal`.

Use `literal: true` for text and command content. Send a separate `keys: "Enter"` when a newline is
needed. Supported key tokens include `C-c`, `C-d`, `Tab`, and `Escape`. Terminal input can affect a
running program immediately; inspect the capture when its current state is unclear. Capture ranges
are inclusive, support negative offsets, and contain rendered plain text.

## Schedules and heartbeats

Use `create_schedule` for cron work that launches a new Agent per run in a fixed workspace.
An external caller supplies `workspaceId` and `provider`; a trusted Agent caller can inherit them.
Resolve the user's timezone to an explicit IANA timezone when wall-clock timing matters; omission
uses UTC. Verify cron, lifetime limits, and any requested initial run before creating it.

Typical argument shapes, with values selected from the user's task and live discovery:

```json
{
  "name": "create_schedule",
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

Replace example IDs and provider/model values with actual results. Use `inspect_schedule` and
`schedule_logs` for status and runs. `run_schedule_once` adds a run without changing cron;
pausing and resuming use their dedicated tools. `update_schedule` takes `id`: `cron` and `every`
are alternatives, and `expiresIn` and `clearExpires: true` are alternatives.

Use `create_heartbeat` only to send recurring prompts into the current, trusted calling Agent.
Ait heartbeats persist across daemon restarts and need explicit `delete_heartbeat` cleanup.
Record the returned ID: `list_schedules` and the ordinary schedule mutation tools operate on
new-Agent schedules, not heartbeats. `delete_heartbeat` requires the original caller's identity.
