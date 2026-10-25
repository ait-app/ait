# OMP ACP feature parity and settlement extensions

This directory contains a source patch for Oh My Pi 18.12.6, revision
`45c35e40c46166dc726ccc52f5d27f40f2449f99` from
[can1357/oh-my-pi](https://github.com/can1357/oh-my-pi).
The upstream source is MIT licensed; see [LICENSE](LICENSE).
Ait itself remains implemented in Rust. The patch changes the external OMP runtime,
not the installed executable automatically.

The patch also makes the ACP session factory honor `--no-session`, so draft discovery
does not create native history.

## Wire contract

- Client opts in through `initialize.clientCapabilities._meta.sessionSettled: true`.
- Agent advertises `initialize._meta.sessionSettled.version: 1`.
- Every opted-in `session/prompt` includes `_meta.turnId`, a nonempty string of at most
  128 characters without ASCII control characters. Invalid identifiers reject before execution.
- A terminal `agent_end` starts an idle barrier, rather than completing the client prompt.
  It waits for admitted submissions, queued input, and owner-scoped async jobs/deliveries.
  Background result turns keep the same ACP forwarding subscription and can start further jobs.
- After the barrier and all event deliveries, the agent flushes the native journal and emits
  `_session/settled` with `{sessionId, turnId}`. The matching successful `session/prompt`
  response follows. Ait accepts either arrival order and requires both before completion.
- Fatal assistant provider errors are carried in `session/prompt.result._meta.error`.
  Streamed error text is tagged with `update._meta.providerError: true`; Ait routes terminal
  failures through its bounded redaction path instead of treating them as assistant prose.
  The final background continuation determines the response, including native failure details.
- Cancelled prompts respond promptly with `stopReason: cancelled`; no quiescence claim is
  made for cancelled work. Ait ends the cancelled host turn from that response.
- Ordinary ACP clients that do not opt in retain bounded prompt-drain behavior and receive
  no settled notifications. Their standard ACP schema remains unchanged.

## Required OMP features

Ait uses ACP for discovery, creation and restoration. It also sends
`initialize.clientCapabilities._meta.omp: 1`; the runtime must advertise
`initialize._meta.omp.version: 1` alongside settlement version 1. Missing extensions
reject before input submission; there is no selectable RPC path or fallback.

- Standard ACP config options expose the four original approval modes, compaction,
  retry and the current model's supported fast/slow toggles. Model choice metadata
  includes native model facts; command metadata retains the skill source.
- `_omp/event` carries native message/tool events and session-owned subagent events.
  Ait's existing OMP projection preserves images, reasoning, token/cost, tool results
  and child timelines. Standard ACP text/tool chunks are not duplicated for opted-in clients.
- `_meta.ompNativeMcp: true` explicitly requests native MCP ownership for real sessions.
  The SDK loads local MCP configuration; an empty ACP server list must not remove it.
  Client-declared servers cannot be mixed with native ownership. Ordinary clients
  keep their original MCP isolation, and Ait's disposable probes do not enable native MCP.
- Existing RPC histories resume via ACP without resubmitting a prompt or changing identity.
  System-prompt changes reopen the same native session over ACP.

## Apply and verify

In a clean checkout of the revision above, apply the absolute path to this patch:

```sh
git apply --check /path/to/ait/third-party/oh-my-pi/acp-session-settled.patch
git apply /path/to/ait/third-party/oh-my-pi/acp-session-settled.patch
bun install --frozen-lockfile
```

Prepare the native addon as documented by OMP (a matching released addon also works), then run:

```sh
bun test packages/coding-agent/test/acp-agent.test.ts packages/coding-agent/test/acp-initialize-conformance.test.ts packages/coding-agent/test/acp-mcp-isolation.test.ts packages/coding-agent/test/agent-session-async-delivery.test.ts
bun run --cwd packages/coding-agent check:types
```

Build/install OMP using its own development instructions, then point `AIT_SERVER_OMP_BIN`
at the resulting launcher before starting Ait. For a source launcher, invoke Bun with the
absolute `packages/coding-agent/src/cli.ts` path and forward all arguments. No root access is
needed for a launcher in the user's home directory. Ait requires the advertised versioned
extension before creating an ACP session; it never replaces it with a timer or silently
falls back to RPC. Existing RPC sessions resume through ACP using the same native history and session identity.
