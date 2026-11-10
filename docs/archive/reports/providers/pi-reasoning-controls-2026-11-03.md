# Pi reasoning controls validation

Validated: 2026-11-03. Source: working-tree changes based on
`0e59bfa27b2ac74585eceee52924fa2ef95fad2b`.
Environment: Linux x86_64, Rust 1.98.1, Node.js 26.8.1, Python 3.14.7,
installed Pi 1.2.0, and Cargo's default features.

## Changes

Pi model discovery advertises native thinking options and seeds per-model defaults from
the current native preference. Optional state-read failures leave the model catalog usable.
Saved model and thinking updates apply before the next prompt. Requested preferences stay
separate from effective native runtime facts, including native clamps and extension changes.

Creation and resume clamp imported or saved thinking preferences to the destination model.
Model changes preserve the requested seed so returning to a supporting model restores it.
Clearing a live override uses a supported Medium baseline when available; dormant clearing
follows restored native state. Explicit native rejections prevent prompt submission and
produce a durable setting-specific failure notice. Bounded state synchronization prevents
ambiguous native thinking changes from being overwritten during admission.

Read-only history import projects the selected branch's recorded thinking preference.
The shared Agent manager captures runtime facts before rejected or busy admission closes
the writer. Provider-owned regressions exercise configuration, discovery, history, restart,
failure persistence, and native notification ordering.

## Validated source

The SHA-256 digest of `git diff HEAD -- crates/provider` is
`36453a80f7f27185626401b4aaf62586fd914f1a61ec9f6a7b492e695ac45b98`.
It identifies the provider source, tests, and RPC fixture changes relative to the base above.

## Validation

- `cargo test --locked -p provider -- --format=terse`: 1,510 unit tests and 2 integration
  tests passed, 0 failed, 63 ignored; provider doctests passed with 0 cases.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored --format=terse`:
  3 passed against installed Pi 1.2.0, 0 failed. This includes native clamping and model-reset
  ordering, Off catalog and request delivery, history import, streaming, images, tools, and resume.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `npm run check:docs`: checker tests passed; local links verified in 510 Markdown documents.
- Pi RPC fixture Python syntax check: passed.
- Changed-document formatting: 10 files passed.
- `git diff --check` and `git diff --cached --check`: passed.

Total: 1,515 Rust tests passed, 0 failed. The 3 installed-Pi tests were enabled separately
from the default suite; the other 60 ignored tests were not executed.

Document formatting command:

```sh
node_modules/.bin/oxfmt --check \
  docs/README.md docs/architecture/README.md docs/decisions/README.md \
  docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/decisions/providers/adr-174-pi-session-import.md \
  docs/decisions/providers/adr-185-native-provider-failure-details.md \
  docs/decisions/providers/adr-211-pi-next-turn-reasoning-controls.md \
  docs/operations/pi.md docs/reports/README.md \
  docs/reports/providers/pi-reasoning-controls-2026-11-03.md
```

## Limits

Runtime validation covers Linux, offline provider fixtures, and installed Pi 1.2.0 with
isolated model configuration and loopback inference. Real model services, other Pi versions,
other operating systems, and browser or native mobile interactions were not exercised.
The installed Off test verifies `reasoning_effort: "none"` for its explicitly mapped
loopback model; native `off` alone does not establish endpoint behavior.

## Test coverage

Coverage was **not measured** because it was not requested. Test counts are execution
results. If requested, use `cargo llvm-cov` for the Pi adapter and related Agent manager
and execution tests, recording the revision, covered/total lines, comparison baseline
when available, and a shared coverage artifact.
