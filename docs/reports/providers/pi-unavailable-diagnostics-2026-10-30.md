# Pi unavailable diagnostics validation

Date: 2026-10-30. Source: working-tree changes based on
`ac8e7e33c71eea3f61c979d566c6cfc1cd5b17dc`.
Environment: Linux x86_64, Rust 1.98.1, installed Pi 1.2.0, and Cargo's default features.

## Changes

Pi diagnostic inspection checks whether the configured executable is runnable before
requesting its version. Missing and non-executable installations return the existing
unavailable guidance without spawning a process or emitting warnings. Each inspection
rechecks the executable, allowing installation or permission repair to recover and
detecting permission loss even after a successful version has been cached.

The regression verifies repeated missing-executable inspection, non-executable files,
installation recovery, permission loss after caching, empty captured logs, and exactly
one successful version probe shared by availability and diagnostic inspection.

[ADR-156](../../decisions/providers/adr-156-pi-native-rpc-provider.md) records the inspection
behavior and references ADR-185 for bounded, redacted native failure details.

## Validation

- `cargo test --locked -p provider --lib local::pi -- --format=terse`:
  126 passed, 0 failed, 1 ignored.
- `cargo test --locked -p provider --lib local::configuration -- --format=terse`:
  7 passed, 0 failed.
- `cargo test --locked -p provider --lib local::diagnostics -- --format=terse`:
  20 passed, 0 failed.
- `cargo test --locked -p provider --lib service::provider_catalog -- --format=terse`:
  27 passed, 0 failed.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored`:
  1 passed against installed Pi 1.2.0 and an isolated loopback model, 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `node --test scripts/check-docs.test.mjs`: 1 passed, 0 failed.
- `node scripts/check-docs.mjs`: local links verified in 485 Markdown documents.
- Changed-document formatting command below: 4 files passed.
- `git diff --check` and `git diff --cached --check`: passed.

Total: 181 Rust tests passed, 0 failed; the normally ignored installed-Pi test was
executed separately.

Changed-document formatting command:

```sh
node_modules/.bin/oxfmt --check \
  docs/README.md docs/reports/README.md \
  docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/reports/providers/pi-unavailable-diagnostics-2026-10-30.md
```

## Limits

Runtime validation covers Linux, offline protocol fixtures, and installed Pi with an
isolated loopback model. Other operating systems and real model services were not exercised.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, use
`cargo llvm-cov` for the affected Pi adapter and related availability, diagnostics,
and catalog tests, recording the revision, covered/total lines, and a shared artifact.
