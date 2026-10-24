# App E2E against Ait

Run from the repository root after `npm install` and installing Playwright Chromium.
Rust/Cargo and Python 3 are required. Global setup builds the frontend dependencies and
`daemon`, warms Metro, then starts one isolated Ait server per Playwright worker.

```sh
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/ait-daemon.spec.ts e2e/browser/workspace-model-restart.spec.ts e2e/browser/daemon-lifecycle.spec.ts
```

Pi import acceptance runs through the production Rust adapter with isolated native JSONL
history and an offline RPC peer; no installed Pi, model service, or credentials are needed.
Its two cases run in desktop and mobile browser viewports.
They require a daemon with the user-image projection from
[ADR-176](../../../docs/decisions/providers/adr-176-pi-user-image-projection.md) and cover:

- Discovery/import with compaction, context edits, and imported image previews, without
  modifying the native history or opening a native session writer.
- Sending a new composer image attachment while continuing the original native session,
  checking the exact image payload in RPC and native JSONL history.
- Image loading at the expected dimensions, saved timeline retention across daemon restart
  and page reload, and explicit refresh after an external history update.
- Retaining both imported and newly sent image previews after refresh, without duplicate
  messages/previews or embedded image payloads in the shared timeline.

Run the focused acceptance suite:

```sh
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/import-session.pi.spec.ts
```

The [2026-10-21 validation report](../../../docs/reports/clients/pi-import-image-e2e-2026-10-21.md)
records the tested source snapshot, commands, results, and setup recovery. The suite does not
validate an installed Pi, a real model service, a native mobile build, or a second physical device.

`E2E_AIT_SERVER_BIN=/absolute/path/to/server` skips the Cargo build. The binary must
implement Ait's v1 API. No installed or published Paseo server is used.

- Workers allocate their own data directory, port, token and provider fixture.
  Server output is retained in that directory as `server.log` until teardown.
- Node helpers use Bearer authentication and the production Rust adapter at `/v1/ws`.
- The browser uses `/v1/auth/ws-ticket` and the production browser transport.
- Test host records use the server's real UUID and the token for that particular port.
- Secondary servers register in a worker-owned connection registry. Seed clients reject
  unknown ports, including local developer services on 6767, 6768 and 7316.
- Default workers use the offline Codex stdio peer from the Rust Provider tests, copied
  into a temporary executable. Pi acceptance also launches isolated daemons with its offline
  Pi RPC peer. These fixtures do not invoke an installed authenticated model CLI.
- `test:e2e:real` explicitly opts into installed providers. `AIT_SERVER_CODEX_BIN` and
  `AIT_SERVER_CLAUDE_BIN` select their executable paths in that mode.
- `E2E_AIT_DATA_ROOT` places worker directories beneath a selected root;
  `E2E_KEEP_AIT_DATA=1` preserves generated state for debugging.
- Playwright traces and recordings may include temporary credentials and remain ignored.

Relay, plugin and historical Paseo server compatibility tests have been removed. All
remaining server launchers target Ait. The broad inherited `browser` project also contains
UI behavior tests that still assume Paseo's mock provider, directory subscriptions,
creation receipts, or old wire-frame shapes; those assumptions need individual migration.
The focused command above is the Ait harness regression, not a claim that every inherited
UI scenario is supported by the server.

Migration details and focused results: [validation report](../../../docs/reports/clients/ait-e2e-migration.md).
