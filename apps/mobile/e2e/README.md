# App E2E against Ait

Run from the repository root after `npm install` and installing Playwright Chromium.
Rust/Cargo and Python 3 are required. Global setup builds the frontend dependencies and
`daemon`, warms Metro, then starts one isolated Ait server per Playwright worker.

```sh
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/ait-daemon.spec.ts e2e/browser/workspace-model-restart.spec.ts e2e/browser/daemon-lifecycle.spec.ts
```

`E2E_AIT_SERVER_BIN=/absolute/path/to/server` skips the Cargo build. The binary must
implement Ait's v1 API. No installed or published Paseo server is used.

- Workers allocate their own data directory, port, token and provider fixture.
  Server output is retained in that directory as `server.log` until teardown.
- Node helpers use Bearer authentication and the production Rust adapter at `/v1/ws`.
- The browser uses `/v1/auth/ws-ticket` and the production browser transport.
- Test host records use the server's real UUID and the token for that particular port.
- Secondary servers register in a worker-owned connection registry. Seed clients reject
  unknown ports, including local developer services on 6767, 6768 and 7316.
- Ordinary tests use the offline Codex stdio peer from the Rust Provider tests, copied
  into a temporary executable. They do not invoke an installed authenticated model CLI.
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

## Migrated UI contracts

The CI `ui-e2e` job runs the offline Codex creation/idempotency/reconnect, streaming
Markdown, delayed File transitions, retained Terminal Find, supported committed-file
Diff, origin-base sidebar statistics and max/ultra reasoning flows. The thinking options
in these tests are fixture-advertised; this validates UI/server configuration propagation,
not the availability or quality of any paid model.

Creation helpers wait for the offline model discovery to finish before submitting.
Wire gates decode Rust request/response correlation through the production adapter aliases;
they do not send historical session envelopes to Rust. The Markdown fixture emits native
Codex deltas and persists the completed message for reload assertions.

Terminal bootstrap is bounded to 12,000 total cells. Find assertions first check that both
needles are actually retained and let initial font refits finish before checking viewport
stability. Control+F tests honor the runner's platform. Commit history enables its list and
base-classification capabilities when Rust advertises the list method; its empty state,
dates, shared layout preferences, reopening and committed-file Diff are covered. Other inherited scenarios outside
this focused set still require individual migration; a skipped feature is not a UI pass.
