# Pi native reasoning controls validation

Date: 2026-10-28. Environment: Linux x86_64, Rust 1.98.1, Node.js 26.8.1,
Vitest 4.1.7, installed Pi 1.2.0, and Cargo default features.

Validated the changes from `8c70b23d` applied to `main` at
`7113431099355c59170bef9b825c92bd9b5d3f23`, including integration resolutions.
The tested source diff is identified by:

```sh
git diff 7113431099355c59170bef9b825c92bd9b5d3f23 -- apps/mobile/src crates/provider/src | sha256sum
# 7327f87469bfe848bf4243a18b81dc0f297921c607b80c33dc86fd0594d8e8c7
```

## Changes

Pi exposes native model-specific thinking choices and catalog defaults. Idle model and thinking
controls apply immediately with native readback; active turns retain durable next-turn intent.
Temporary configuration sessions close after the operation, preserving Agent status and saved
preferences. Runtime facts, effective configuration, and changed native handles persist together,
with retries after storage failure and reconciliation after partial native success.

Shared client controls, facts, creation forms, and profile forms consume provider presentation
policy. Pi displays effective native values, preserves unknown native thinking labels, and shows
queued selections separately from the current turn's runtime facts.

The control and persistence contract is recorded in
[ADR-198](../../decisions/providers/adr-198-pi-reasoning-controls.md). Operational behavior is
documented in the [Pi guide](../../operations/pi.md).

## Validation

| Command                                                                          | Result                                                                     |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cargo test --locked -p provider`                                                | Final run: 1,416 passed, 0 failed, 60 ignored; provider doctests: 0 cases. |
| `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored` | 5 passed, 0 failed, 1,471 filtered out; installed Pi 1.2.0.                |
| Daemon integration command below                                                 | 4 passed, 0 failed, 100 filtered out.                                      |
| `cargo test --locked -p daemon --test dependencies`                              | 16 passed, 0 failed.                                                       |
| Related client command below                                                     | 322 passed across 25 files, 0 failed.                                      |
| `npm run build:sdk`                                                              | Passed.                                                                    |
| `npm run typecheck --workspace=@ait/mobile`                                      | Passed without diagnostics.                                                |
| `cargo build --locked --workspace`                                               | Passed without warnings.                                                   |
| `cargo clippy --locked --workspace --all-targets -- -D warnings`                 | Passed without warnings.                                                   |
| `cargo fmt --all --check`                                                        | Passed.                                                                    |
| `npm run check:ui`                                                               | 13 changed files; 0 warnings, 0 errors.                                    |
| Changed-file formatting command below                                            | Passed.                                                                    |
| `npm run check:docs`                                                             | 1 checker test passed; local links verified in 476 Markdown documents.     |
| `git diff --check` and `git diff --cached --check`                               | Passed.                                                                    |

The initial provider run reported 1,415 passed, 1 failed, and 60 ignored. The failed DSH
`refused_loopback_connection_records_the_connect_stage` assertion passed in isolation and in
the full provider rerun, without source changes or reduced parallelism. Its isolated command was:

```sh
cargo test --locked -p provider \
  local::deepseek_harness::native::http::tests::transport::refused_loopback_connection_records_the_connect_stage \
  --lib
# 1 passed, 0 failed, 1,475 filtered out.
```

Daemon integration command:

```sh
cargo test --locked -p daemon --test process -- \
  session::config_and_attention_streams_are_connection_owned_and_presence_controls_notifications \
  agent_controls:: bots::
```

Related client command:

```sh
npm run test --workspace=@ait/mobile -- \
  src/provider-selection src/agent-profiles src/composer/agent-controls \
  src/panels/agent-facts-line.test.ts src/contexts/session-context \
  src/agent-controls src/command-center/agent-control-contributions.test.ts \
  src/components/model-browser-view.test.ts src/stores/session-store
```

Changed-file formatting:

```sh
git diff --name-only --diff-filter=ACMR main -- apps/mobile docs | \
  rg '\.(tsx?|md)$' | xargs node_modules/.bin/oxfmt --check
```

## Limits

Installed-Pi cases use isolated native settings and loopback model endpoints. No real model
service, browser acceptance suite, native mobile build, or macOS/Windows execution was tested.
No full Rust workspace test suite was run.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, use `cargo llvm-cov`
for the affected Pi adapter and shared configuration/persistence behavior, recording the revision,
features, covered/total lines, baseline, exclusions, and a shared artifact. Passing test counts
above are execution results.
