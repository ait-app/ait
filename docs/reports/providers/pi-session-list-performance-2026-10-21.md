# Pi session-list latency investigation

Date: 2026-10-21. Investigation baseline: `1c0fddc32ae58f4f6f2e3adf74395de8258f7c16`.
The first fix was subsequently implemented and validated in the working tree on that base;
the original measurements below describe behavior before the fix.

## Finding

Before the fix, session listings from different providers blocked one another on the app's shared WebSocket
channel. This reproduces the reported pattern: Codex rows appear quickly, while selecting
Pi shows no rows for several seconds even though Pi's actual scan is fast.

The controlled comparison used the same daemon, native store, limit of 15, and overlapping
requests. Only the client connection carrying the Pi request changed:

| Request                                      | Client-observed latency | Entries |
| -------------------------------------------- | ----------------------: | ------: |
| OpenCode listing on the shared connection    |             10,609.7 ms |      15 |
| Pi queued behind OpenCode on that connection |             10,651.2 ms |      15 |
| Pi on a separate connection at the same time |                 17.6 ms |      15 |

Backend execution times were 10,607 ms for OpenCode, 28 ms for the queued Pi request,
and 15 ms for the independent Pi request. The queued Pi request reported `queue_ms=1`:
almost all its client-visible delay happened before the WebSocket reader admitted it to
the provider worker. Provider-worker queue metrics alone therefore miss this wait.

This establishes the mechanism in the measured environment. It does not establish which
provider occupies the channel on the user's running host, or reproduce exactly five seconds.
Any preceding slow request on that channel can produce the same behavior.

## Request path

1. The [import sheet](../../../apps/mobile/src/components/import-session-sheet.tsx)
   creates a query for every eligible provider. Selecting Pi only filters already-collected
   rows; the selected provider does not participate in the query configuration and does not
   prioritize its pending request. Pi is offered in the filter after its catalog status leaves
   `loading`, although its session rows may still be pending.
2. The [Rust client method mapping](../../../apps/mobile/src/runtime/rust-daemon/methods.ts)
   sends every `provider.sessions.recent.list.request` on channel 0. Provider snapshot requests
   use channel 0 too.
3. The [ordinary WebSocket reader](../../../crates/api/src/connection.rs) awaits
   `process_message`, which awaits the request handler before reading the next message.
4. [Provider execution dispatch](../../../crates/provider/src/dispatch/requests.rs) awaits
   session listing through `agent_execution::dispatch`, then sends its response. Listing is
   absent from the methods using the independent, bounded
   [catalog response path](../../../crates/provider/src/dispatch/catalog.rs).
5. With a populated provider snapshot in this environment, the sheet sends Codex, OpenCode,
   and Pi listings in that order. UI query concurrency does not make these requests execute
   concurrently on the shared connection.

Two complete runs through the app's SDK, Rust transport, snapshot codec, and snapshot cache
reproduced the ordering after catalog discovery was complete:

| Run | Codex rows available | OpenCode rows available | Pi rows available | Pi backend execution |
| --: | -------------------: | ----------------------: | ----------------: | -------------------: |
|   1 |             194.9 ms |             10,755.6 ms |       10,799.3 ms |                37 ms |
|   2 |             254.1 ms |             10,918.6 ms |       10,964.2 ms |                44 ms |

A subsequent Pi-only request took 54.5 ms. The harness did not mount React or measure rendering.
It used in-memory storage for the real snapshot-cache implementation and omitted icon updates.

## Why OpenCode can occupy the channel

The [OpenCode client](../../../crates/provider/src/local/opencode/client.rs) waits for native
listing, process closure, and prompt-preview enrichment before returning descriptors.
[Preview enrichment](../../../crates/provider/src/local/opencode/history/previews.rs) starts
native history loads with concurrency four, a three-second timeout per history, and a ten-second
overall deadline. The measured OpenCode listing execution of approximately 10.6 seconds is
consistent with that deadline plus native startup and listing overhead. It is not a separately
instrumented measurement of each OpenCode substage.

This also explains why earlier probes missed the problem: they requested Pi and Codex
individually or loaded their lists as their catalogs became ready. They did not enqueue
OpenCode's session listing before Pi on the same connection.

## Pi scan costs are a separate, reproducible issue

A temporary standalone profiler extracted the current Pi listing implementation through
`list_before`, added aggregate stage timers, and compiled against the checkout's existing debug
dependency artifacts. It did not start Pi, import sessions, materialize images, or change native
files. These are instrumented extraction measurements, not executions of a newly built provider
crate. The profiler ran without optimization, matching the checkout's development profile;
release performance was not measured. Filesystem caches were not flushed.

The actual default Pi store contained 45 files totaling 7.07 MiB:

| Scan limit | Parsed files | Parsed data | Total | JSON parsing | Timestamp validation | Cwd recovery |
| ---------: | -----------: | ----------: | ----: | -----------: | -------------------: | -----------: |
|         15 |           15 |    1.00 MiB | 13 ms |        10 ms |              0.24 ms |      0.03 ms |
|        500 |           45 |    7.07 MiB | 84 ms |        67 ms |              1.44 ms |      0.08 ms |

Synthetic stores demonstrate the scaling problem independently of native startup or transport:

| Synthetic shape                | Scan limit | Parsed files | Parsed data |    Total | Stop condition |
| ------------------------------ | ---------: | -----------: | ----------: | -------: | -------------- |
| Large tool text strings        |         15 |           15 |   60.01 MiB |   207 ms | Result limit   |
| Large inline image strings     |         15 |           15 |   60.01 MiB |   200 ms | Result limit   |
| Many small tool content blocks |         15 |           15 |    9.86 MiB |   653 ms | Result limit   |
| Same tool-block store          |        115 |           50 |   32.88 MiB | 2,035 ms | Deadline       |
| Many short message records     |         15 |           15 |   12.81 MiB |   774 ms | Result limit   |
| Same short-message store       |        115 |           39 |   33.30 MiB | 2,006 ms | Deadline       |

The scan-limit comparison models the effect of 100 additional imported-handle keys on a request
for 15 rows. It does not create 100 imported Agents. The generated native records exercise
listing and do not claim valid images or importable complete histories.

Object density matters more than byte count alone: decoding large plain base64 strings was
much cheaper than allocating many small JSON objects. In the 2,035 ms tool-block run, JSON
parsing consumed 1,666 ms, filesystem reading 26 ms, and timestamp validation less than 1 ms.
Allocation cleanup and remaining work account for much of the unassigned time. In the short-
message run, branch construction, preview generation, and timestamps also contributed.

Code review confirms the following additional limitations:

- Full JSON parsing is repeated for each listing. There is no descriptor cache across requests.
- The scan limit includes imported-handle keys from every provider, including distinct native
  aliases. The eight-times candidate limit is an upper bound; listing stops earlier when its
  result, byte, or elapsed budget is reached. Files are enumerated and sorted before this bound.
- The two-second deadline cannot interrupt parsing a file. The 64 MiB budget counts whole file
  sizes when headers are opened, even when a subsequent cwd check skips the body. An upcoming
  file exceeding the remaining byte budget ends the scan rather than trying smaller later files.
- Default-layout cwd recovery is cached only within a request. Its filesystem search can be
  expensive on ambiguous paths, but it was negligible in the measured local store.
- Claude's current listing also reads and parses complete candidate histories; it is not a
  header-only baseline. Codex uses native `thread/list` metadata.
- Successful Pi version probes are cached; failed probes can add repeated delays. Catalog
  startup is an additional cost before the UI makes Pi selectable, not the cause demonstrated
  by the same-connection versus separate-connection comparison.

## Fix priority

1. Give native session listing an independently tracked, bounded response path so the physical
   WebSocket reader can admit the next request immediately. Retain the existing shared history
   admission limit. The catalog response path is a useful precedent; broad concurrency for
   lifecycle mutations is not required. Cover both ordinary and single-connection transports.
2. Avoid holding up OpenCode descriptors for best-effort prompt previews, or cache these previews
   so each first import-panel query does not repeat native history loads.
3. Cache Pi descriptors using file identity, size, and modification metadata, with invalidation
   when native files change. Scope imported-handle allowances to the relevant provider; preserve
   alias filtering and the ability to fill the requested number of unimported results.
4. Reduce Pi listing allocations while preserving active-branch/title/preview semantics. Retain
   full strict parsing for inspection. Do not replace tree-aware discovery with naive head/tail
   sampling or remove original-cwd recovery without preserving symlink lookup behavior.

Selecting Pi earlier or optimizing only its timestamps would leave the demonstrated shared-
connection delay intact.

## Implemented first fix

Native session listing now uses the existing bounded catalog response-admission path, as
specified by [ADR-180](../../decisions/providers/adr-180-native-session-list-response-admission.md).
The connection dispatcher returns after synchronous admission and registers a tracked response
task. Command capacity, response capacity, history admission, request correlation, and native
session lifecycle ordering retain their existing bounds and semantics.

The same all-provider SDK reproduction was rerun after `cargo build -p daemon --bin daemon`
against this working tree. Catalog discovery completed before each panel query, matching the
original reproduction. Each row reports elapsed time from requesting all provider lists:

| Run | Pi before fix | Pi after fix | OpenCode after fix |
| --: | ------------: | -----------: | -----------------: |
|   1 |   10,799.3 ms |      22.7 ms |        10,782.6 ms |
|   2 |   10,964.2 ms |      31.3 ms |        10,696.2 ms |

Each listing returned 15 rows. Pi backend execution was 17 ms and 20 ms; its rows arrived while
OpenCode was still loading. Native preview behavior and Pi scan algorithms were not changed.
This is local SDK/transport evidence, not a measurement of React rendering or another host.

The import sheet also keeps displayed rows in place while appending results from slower
providers. Existing descriptors update in place and absent rows are removed. Session query
keys, refreshes, and provider retries include the host ID; display ordering resets when the
host, directory, search query, or sheet visibility changes.

## Validation

Rerun date: 2026-10-22. Validated the uncommitted working tree based on
`1c0fddc32ae58f4f6f2e3adf74395de8258f7c16`, including the provider admission changes,
transport regressions, and import-sheet ordering and host-cache changes. These results
supersede the earlier focused test results; the latency tables above remain the original
2026-10-21 measurements and were not rerun for this validation.

Environment: Linux x86_64, Rust and Cargo 1.98.1, Node.js 26.8.1, npm 12.0.2,
and Vitest 4.1.7. Rust commands used default features and normal test parallelism.

All focused suites passed: **47 Rust tests and 88 UI tests, 0 failed, 0 ignored/skipped**
in the successful runs. Unrelated tests were excluded by the filters below.

| Command                                                                                                                                       | Passed | Test duration |
| --------------------------------------------------------------------------------------------------------------------------------------------- | -----: | ------------: |
| `cargo test -p provider --lib native_sessions`                                                                                                |     38 |        0.53 s |
| `cargo test -p provider --lib service::agent_execution::tests::subscriptions::catalog`                                                        |      6 |        0.23 s |
| `cargo test -p daemon --test process agent_history::native_sessions`                                                                          |      2 |        0.21 s |
| `cargo test -p provider --lib capabilities::tests::installation_selects_the_entire_crate`                                                     |      1 |        0.01 s |
| `npm run test --workspace=@ait/mobile -- src/components/import-session-sheet-view-model.test.ts src/components/import-session-sheet.test.tsx` |     88 |        8.01 s |

Rust durations are the test harness execution times, excluding Cargo startup/build time.
The UI duration is Vitest's total reported duration for both files.

The new provider regressions cover immediate dispatch return during gated native I/O,
independent listing and validation-error replies with original request IDs, all 64 response
slots retained after computation, and draining accepted work after disconnect. The two daemon
tests cover actual ordinary and single-connection WebSocket transport: a second session-list
response must arrive while the first native listing is held behind a deterministic gate.

The UI tests cover stable ordering when a slower provider returns newer sessions, descriptor
updates and removals, unchanged-array identity, and host-isolated cached rows and refreshes.
The host-switch test uses the same provider and directory with a shared query cache.

The 38-test filter passed outside the sandbox. Both daemon transport tests ran outside the sandbox to allow
isolated daemon listeners. No assertions were relaxed and no source changes were needed.

Additional checks passed during this rerun:

- `cargo fmt --all --check`.
- `cargo clippy --locked -p provider -p daemon --all-targets -- -D warnings`.
- `cargo build -p daemon --bin daemon`.
- `npm run typecheck --workspace=@ait/mobile`, without diagnostics.
- `node_modules/.bin/oxlint --deny-warnings` for the four changed import-sheet source/test
  files, with zero warnings or errors.
- `node_modules/.bin/oxfmt --check` for the four changed import-sheet files, ADR-180,
  this report, and the three changed documentation indexes.
- Python syntax parsing of `crates/provider/tests/fixtures/codex_app_server.py`.
- `npm run check:docs`: documentation checker tests passed and local links verified in
  432 Markdown documents.
- `git diff --check`.

No full Rust workspace suite, live-provider latency probe, browser acceptance suite, native
mobile build, or macOS/Windows validation was run. UI validation uses component and view-model
tests; transport tests use deterministic native fixtures rather than a real model service.

## Test coverage

Coverage was **not measured** because it was not requested. The passing test counts above
report execution results, not line or branch coverage. If requested later, measure the changed
provider admission/dispatch code with focused `cargo llvm-cov` tests and publish the report
with its exact revision and measurement scope. No coverage percentage, baseline comparison,
or instrumented coverage artifact is claimed by this report.
