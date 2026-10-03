# Local 0.0.7 subscription and live-session compatibility

Date: 2026-09-27. Source: working tree based on `b733bc0`.

## Confirmed causes

1. `terminal.list.subscribe.request` was translated to `list_terminals_response`.
   The SDK's `observeTerminals` waits for `terminals_changed`, including the owned
   subscription ID. The unrecognized bootstrap reply left its waiter pending for
   60 seconds. Timeout closed the shared transport with `Subscription request
failed`, also interrupting unrelated requests.
2. Rust agent snapshots serialize an absent `lastError` as `null`. The frontend
   accepted only a string or an omitted field, rejecting populated agent-directory
   responses every five seconds. The captured validation path was
   `message.payload.entries[n].agent.lastError`.
3. Live sessions additionally expose `persistence.nativeHandle` and
   `persistence.metadata` as `null`. DevTools on the installed local build captured
   both validation failures in `fetch_agents_response`. The earlier metadata-only
   test used cold snapshots and missed this difference. The Rust runtime-info
   contract also permits a null `extra` record, which now has the same handling.

The terminal failure reproduced in an isolated profile on the original installed
release. After the first local rebuild, the installed application's bundle matched
that rebuild, yet its running-session snapshots still failed validation. This
rules out a stale application process as the cause of the remaining list error.

## Changes

- Map terminal subscription bootstrap replies to `terminals_changed`.
- Normalize null `lastError`, persistence fields and runtime extras to the existing
  optional SDK representation. Preserve populated values and reject malformed
  values; no session records or message history are migrated or rewritten.
- Test terminal bootstrap, pushed updates, ownership/release and connection
  survival through the SDK and adapter.
- Test null, omitted, populated and malformed snapshot fields with both Zod and
  the generated validator, including a populated live-session directory.
- Include protocol tests in desktop CI.
- Extend the packaged startup smoke test with an isolated offline provider peer:
  create a workspace/session, subscribe to its timeline, send a streaming turn,
  read the running session list, steer the turn, receive tool/stream events,
  finish, release subscriptions and recover persisted history after restart.
  Fail on renderer protocol-validation warnings as well as uncaught errors.

## Validation

- Protocol: 734 tests passed (65 files).
- Client: 216 tests passed (8 files; network E2E suites excluded).
- Rust transport adapter: 31 tests passed (3 files).
- Release packaging: 12 tests passed.
- SDK build, app/client type checks, method catalog check, formatting and lint
  passed. Catalog check: 171 frontend mappings to 168 canonical Rust methods.
- Fixed SDK/adapter with the installed Rust binary and temporary metadata copies:
  terminal subscription/release and queries for two agents and one workspace
  passed. Read-only timeline subscriptions and native-history loading returned
  22 and 174 entries for the two existing agents. No message was sent to either
  user session and no original application registry was modified.
- A separate isolated offline-provider check passed create, subscribe, send,
  steer, completion, directory and timeline reads, including reasoning and tool
  events. This verifies the complete client/adapter/server protocol path without
  making a paid model request.
- Final packaged-app check with isolated user metadata returned two agents and
  one workspace, completed the terminal subscription in 1 ms, and reported zero
  validation warnings through the next directory refresh.

## Test coverage

Rust coverage: not applicable — no Rust source changed. Per repository policy,
Rust workspace tests and llvm-cov were not rerun. The release Rust binary was
built as part of desktop packaging. TypeScript coverage percentage was not
measured; test counts above are execution results, not coverage percentages.

## Local artifact

Built with `EXPO_NO_TELEMETRY=1 EXPO_OFFLINE=1 AIT_DESKTOP_SMOKE=1
AIT_DISABLE_SINGLE_INSTANCE_LOCK=1 npm run build:dmg`. The extended packaged
startup smoke passed all live-session and restart checks with zero renderer
errors. The app passed `codesign --verify --deep --strict` and the DMG passed
`hdiutil verify`. The local app uses an ad-hoc signature and is not notarized.

Artifact: `apps/desktop/release/Ait-0.0.7-local-arm64.dmg`, with a neighboring
`.sha256` file. SHA-256:
`2c89c48edf56f819f3ea89adc8c8691aef78e4bc9c524c29999ba114f10c9a86`.

This package includes the new live-session null-field fixes. It replaces the
earlier local artifact, whose metadata-only verification missed those fields.
Publishing release assets and replacing the user's installed application remain
separate operations.
