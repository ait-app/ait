# Pi unavailable diagnostics validation

Validated: 2026-10-31. Source: working-tree changes based on
`5fdc2c96b9b08c8a3dc8bc89a487643d687cf51d`.
Environment: Linux x86_64, Rust 1.98.1, Node.js 26.8.1, installed Pi 1.2.0,
and Cargo's default features.

## Changes

Pi diagnostic inspection checks whether the configured executable is runnable before
requesting its version. Missing and non-executable installations return the existing
unavailable guidance without spawning a process or emitting warnings. Each inspection
rechecks the executable, allowing installation or permission repair to recover and
detecting permission loss even after a successful version has been cached.

Successful versions remain cached per configured client and its clones. Failed probes
share a five-second cooldown and a five-second probe timeout. Classified failures use
the client's shared 30-second log aggregation window; actual spawn failures also retain
safe OS error facts. Per-Agent environment overrides have independent probe state.

Executable-check failures alone return PATH guidance. A file that passes those checks
but cannot start gets a distinct launch-failure message with the safe OS error kind and
optional numeric code. The probe cache retains these typed facts without paths, arguments,
or native error text. Shared session errors still classify launch failures as unavailable.

Regressions cover quiet executable inspection, installation and permission recovery,
permission loss after success caching, concurrent failure reuse, cooldown expiry,
recovery after repair, bounded catalog refreshes, spawn-error facts, quiet unsupported
versions, and independent environment overrides. Launch regressions cover missing interpreters,
invalid executable formats, safe permission-error formatting with and without an OS code,
catalog-visible guidance, cached messages across clones, and recovery after repair.
The installed-Pi test exercises discovery,
streaming, tools, images, session persistence, and resume with a loopback model.

[ADR-156](../../decisions/providers/adr-156-pi-native-rpc-provider.md) records the inspection
behavior and references ADR-185 for bounded, redacted native failure details.

## Validated source

SHA-256 hashes identify the Rust files used for validation:

| File                                                | SHA-256                                                            |
| --------------------------------------------------- | ------------------------------------------------------------------ |
| `crates/provider/src/local/pi.rs`                   | `877eb4aeb259328a813e2666042763b028009f769990341a6417d226570a3c19` |
| `crates/provider/src/local/pi/transport.rs`         | `46b01aedae51975e8f3f081085ff63c70df930d311358f7f4956f07d4461704c` |
| `crates/provider/src/local/pi/tests/diagnostics.rs` | `73c583f7c3fd282d183df3a30ea703debb561453335038c2092d9a620311f1bf` |
| `crates/provider/src/local/pi/tests/review.rs`      | `c0afd70ff84695a44f115ddb6b5a8bea6c779037aa9327e92670dc7c13d0ee19` |
| `crates/provider/src/local/pi/transport/tests.rs`   | `eb6c3c4602ba5795d179ac8f98dadb274a97d3fb4cdeebdd1c43113d345fb2a1` |

## Validation

- `cargo test --locked -p provider --lib -- local::pi local::configuration local::diagnostics service::provider_catalog service::agent_execution service::agent_manager ports::agent_session --format=terse`:
  406 passed, 0 failed, 1 ignored.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored`:
  1 passed against installed Pi 1.2.0 and an isolated loopback model, 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `node --test scripts/check-docs.test.mjs`: 1 passed, 0 failed.
- `node scripts/check-docs.mjs`: local links verified in 489 Markdown documents.
- Changed-document formatting command below: 6 files passed.
- `git diff --check` and `git diff --cached --check`: passed.

Total: 407 Rust tests passed, 0 failed; the normally ignored installed-Pi test was
executed separately.

Changed-document formatting command:

```sh
node_modules/.bin/oxfmt --check \
  docs/README.md docs/reports/README.md \
  docs/decisions/providers/adr-144-provider-error-reporting.md \
  docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/operations/pi.md \
  docs/reports/providers/pi-unavailable-diagnostics-2026-10-30.md
```

## Limits

Runtime validation covers Linux, offline protocol fixtures, and installed Pi with an
isolated loopback model. Other operating systems and real model services were not exercised.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, use
`cargo llvm-cov` for the affected Pi adapter and related availability, diagnostics,
and catalog tests, recording the revision, covered/total lines, and a shared artifact.
