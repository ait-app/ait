# Pi admission lifecycle validation

Validated: 2026-11-08. Source: working-tree changes based on
`9d925ac5e2d1efadedb57f6a1486587422751668`.
Environment: Linux x86_64, Rust 1.98.1, installed Pi 1.5.0, and Cargo's default features.

## Changes

Fatal or uncertain prompt admission retires the Pi writer, finalizes pending tools and
compactions, restores normal polling budgets and retains the original diagnostic. Submitted
input precedes observed native work in the adapter queue when acknowledgement fails;
definitive rejection does not project the rejected input or count concurrent extension work
as acceptance. The adapter can continue after rejection; the shared manager closes its writer
on admission errors and opens another writer for a later user turn.

Persistence distinguishes empty sessions, observed native work and required native history.
`observedWork` survives manager checkpoints and is promoted to `hasInput` when a bounded
version-three header check verifies the UUID and canonical cwd. Checks run after admission,
after process shutdown and before resume, preserving history written during cleanup.
Missing filenames use project-scoped lookup; an existing native-file requirement is never
cleared. Header checks preserve the existing regular-file, symlink and store-containment rules.

Repeated transport close calls finish shutdown and reader cleanup after cancellation, and
report a retained shutdown error once. ADR-156, ADR-174 and the Pi operations guide describe
the resulting lifecycle and resume metadata.

## Validated source

The SHA-256 digest of `git diff HEAD -- crates/provider` is
`e7368ea4bc6011512a58753af7cd79d143a2adcadeea3606b302adea79c54474`.
It identifies the provider source, tests and Python fixture relative to the base above.

## Validation

- `cargo test --locked -p provider -- --format=terse`: 1,663 unit tests and 2 integration
  tests passed, 0 failed, 64 ignored; provider doctests passed with 0 cases.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored --format=terse`:
  4 passed against installed Pi 1.5.0, 0 failed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `npm run check:docs`: 1 checker test passed; local links verified in 531 Markdown documents.
- Python fixture syntax: 1 file passed using the command below.
- Changed-document formatting: 6 files passed using the command below.
- `git diff --check` and `git diff --cached --check`: passed.

Total: 1,669 Rust tests passed, 0 failed. The 4 installed-Pi tests were enabled separately;
the other 60 ignored tests were not executed.

Python syntax command:

```sh
python3 - <<'PY'
from pathlib import Path
filename = 'crates/provider/src/local/pi/tests/fixtures/pi.py'
compile(Path(filename).read_text(), filename, 'exec')
PY
```

Document formatting command:

```sh
node_modules/.bin/oxfmt --check \
  docs/README.md docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/decisions/providers/adr-174-pi-session-import.md docs/operations/pi.md \
  docs/reports/README.md docs/reports/providers/pi-admission-lifecycle-2026-11-08.md
```

## Limits

The shared manager's admission-error path does not drain the adapter's queued input and work
into the durable timeline. Explicit native-history refresh can recover saved work that passes
full inspection. Header verification proves identity only; it does not prove that a complete
prompt or history body was saved, and shutdown does not guarantee a flush.

Runtime checks cover Linux, offline native-process fixtures and installed Pi 1.5.0 with
temporary resources and loopback model services. Other operating systems, real model services,
other Pi versions and browser or mobile interactions were not exercised.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, measure the Pi
adapter and related provider services with `cargo llvm-cov`, recording the source revision,
covered/total lines, a comparable baseline when available and a shared artifact.
