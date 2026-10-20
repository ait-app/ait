# Pi native session import validation

Date: 2026-10-20. Validated the Pi session import working tree on branch
`feat/pi-native-session-import`, based on `974c64f3`, with default Rust features.
Focused validation covers the complete staged implementation, including resume metadata and
shared history admission regression tests. The full workspace results below retain the exact
snapshot and counts from that run.

This follows the native RPC adapter introduced in [PR #357](https://github.com/ait-app/ait/pull/357).
The import boundary is recorded in [ADR-174](../../decisions/providers/adr-174-pi-session-import.md);
supported behavior and storage configuration are documented in the
[Pi operations guide](../../operations/pi.md). The [original adapter report](pi.md)
describes an earlier revision and is not evidence for this run.

## Environment

- Linux x86_64.
- Rust: `rustc 1.98.1 (48a229cea 2026-09-01)` (Arch Linux package `1:1.98.1-1`).
- Cargo: `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
- Installed Pi: `1.2.0`.
- The recorded full Rust workspace test run was explicitly requested by the user.

## Focused provider tests

All commands use `CARGO_INCREMENTAL=0`. On the current working tree, **165 test executions
passed, 0 failed** across these successful runs:

| Command                                                                 |  Passed | Ignored |
| ----------------------------------------------------------------------- | ------: | ------: |
| `cargo test -p provider --lib local::pi`                                |     122 |       1 |
| `cargo test -p provider --lib service::agent_execution::tests::resume`  |       7 |       0 |
| `cargo test -p provider --lib native_sessions`                          |      34 |       0 |
| `cargo test -p provider --lib local::opencode::tests::resume`           |       1 |       0 |
| `cargo test -p provider local::pi::tests::installed --lib -- --ignored` |       1 |       0 |
| **Total**                                                               | **165** |   **1** |

The Pi installed-runtime case is skipped by the regular Pi filter and explicitly executed
separately. The `native_sessions` filter also matches shared protocol and other provider tests;
its count is not Pi-only coverage. Initial sandbox runs of that filter and the installed-Pi
case could not bind loopback listeners; both passed when rerun outside the sandbox.

Regression coverage includes replacing unverified resume metadata with inspection's verified
facts while retaining configuration overrides, preserving OpenCode client-message correlations,
and shared history capacity under saturation, cancellation, availability errors, and successful
listing. All five newly added tests passed in these runs.

## Full workspace tests

The recorded full workspace run validated snapshot
`604f66566699bca1f04c932c8707b176a1560b1f`. It was not rerun for this update;
its totals are separate from the current working-tree focused executions above.

Command:

```sh
CARGO_INCREMENTAL=0 cargo test --workspace
```

The command ran outside the sandbox with local-listener permissions and exited with
status **0**: **2,586 passed, 0 failed,
40 ignored**, with no test-name filters. All 20 unit/integration test targets passed.
All 14 doctest targets completed successfully; they defined zero doctest cases.

| Target                                |   Passed | Failed | Ignored |
| ------------------------------------- | -------: | -----: | ------: |
| api unit tests                        |      112 |      0 |       0 |
| api tests/fragmentation.rs            |        1 |      0 |       0 |
| bot unit tests                        |       35 |      0 |       0 |
| browser unit tests                    |       44 |      0 |       0 |
| daemon unit tests                     |       76 |      0 |       0 |
| daemon tests/dependencies.rs          |       16 |      0 |       0 |
| daemon tests/process.rs               |      105 |      0 |       0 |
| domain unit tests                     |       24 |      0 |       0 |
| filesystem unit tests                 |      377 |      0 |       0 |
| filesystem tests/module_boundaries.rs |        2 |      0 |       0 |
| host_link unit tests                  |       15 |      0 |       0 |
| metadata unit tests                   |      247 |      0 |       0 |
| metadata tests/module_boundaries.rs   |        2 |      0 |       0 |
| model unit tests                      |       84 |      0 |       0 |
| persistence unit tests                |       76 |      0 |       0 |
| provider unit tests                   |     1061 |      0 |      39 |
| relay unit tests                      |       46 |      0 |       0 |
| schedule unit tests                   |       82 |      0 |       0 |
| terminal unit tests                   |      103 |      0 |       1 |
| voice unit tests                      |       78 |      0 |       0 |
| **Total**                             | **2586** |  **0** |  **40** |

The 40 ignored cases comprise 39 optional provider tests and one fish shell compatibility
test. These are not counted as passing. The Pi installed-runtime case is invoked separately
below; the remaining optional installed-provider and fish tests were not run.

## Installed Pi integration

Command: `CARGO_INCREMENTAL=0 cargo test -p provider local::pi::tests::installed --lib -- --ignored`.

Passed: **1 passed, 0 failed**, exit status **0**. This is an additional execution,
included in the focused total above and separate from the workspace totals. This optional test uses Pi 1.2.0, isolated temporary configuration,
and a loopback fake model; it does not require a real model service or user credentials.

## Documentation checks

Report formatting, local links in all 412 Markdown documents, and both staged and unstaged
diff whitespace checks passed:

```sh
node_modules/.bin/oxfmt --check docs/reports/README.md docs/reports/providers/pi-session-import-2026-10-20.md
node scripts/check-docs.mjs
git diff --check
git diff --cached --check
```

## Behavior and limits

Pi session import adds bounded host-store discovery, active-branch history projection,
explicit refresh, and continuation in the original native session. Projection includes
retained context edits, compaction summaries, tools, reasoning, and images. Native
lookup spelling is preserved separately from canonical Agent placement.

The imported timeline reflects retained model context and can differ from Pi's displayed
transcript. External Pi writers must be stopped before import or continuation. Discovery
cannot infer arbitrary project-local custom stores during unfiltered listing. Automatic
history replay, thinking controls, and rewind remain unsupported.

## Test coverage

Coverage was not measured. Test counts describe executed tests, not line or branch
coverage. This report does not claim validation of other platforms, real model services,
or other installed Pi versions. Cargo's default workspace test run leaves explicitly
ignored tests unexecuted unless they are invoked separately.
