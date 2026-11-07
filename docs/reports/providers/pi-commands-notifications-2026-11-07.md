# Pi slash commands and extension notifications validation

Validated: 2026-11-07. Source: working-tree changes based on
`6bd639f6ce22c8d3d020232167762e1f0ab54e82`.
Environment: Linux x86_64, Rust 1.98.1, installed Pi 1.5.0, and Cargo's default features.

## Changes

Pi discovers native extension commands, prompt templates and skills through ephemeral
`get_commands` requests, preserving invocation names and bounding catalog payloads.
Each backend command request uses a fresh process. Discovery shares lifecycle handling
with model probes and retains a received RPC result when process cleanup fails.

Active-turn extension notices become persisted timeline notifications with bounded
metadata, cleaned terminal formatting and separate queue allowances. Notices yield
capacity to required turn output. Submitted input retains its position after surviving
prior-turn output, and native history import uses its own entry limit.

## Validated source

The SHA-256 digest of `git diff HEAD -- crates/provider` is
`c4ed80a5cae0c5d3b67ebde095b27a9d9abc0b95d8986e42201816097074d278`.
It identifies the provider source, tests and Python fixtures relative to the base above.

## Validation

- `cargo test --locked -p provider -- --format=terse`: 1,635 unit tests and 2 integration
  tests passed, 0 failed, 64 ignored; provider doctests passed with 0 cases.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored --format=terse`:
  4 passed against installed Pi 1.5.0, 0 failed. These cover native command discovery,
  extension invocation, prompt-template and skill expansion, project trust, thinking
  controls, streaming, images, tools, history import and resume.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `npm run check:docs`: 1 checker test passed; local links verified in 527 Markdown documents.
- Python fixture syntax: 2 files passed using the command below.
- Changed-document formatting: 9 files passed using the command below.
- `git diff --check` and `git diff --cached --check`: passed.

Total: 1,641 Rust tests passed, 0 failed. The 4 installed-Pi tests were enabled separately;
the other 60 ignored tests were not executed.

Provider regressions include command mapping and payload limits, fresh probes and failure
recovery, version caching, discovery cleanup, notification floods and ordering, timeline
reopen persistence, input capacity and large native-history messages. The crate suite also
covers shared discovery, session management, execution, ports and timeline storage.

Python syntax command:

```sh
python3 - <<'PY'
from pathlib import Path
for filename in [
    'crates/provider/src/local/pi/tests/fixtures/pi.py',
    'crates/provider/src/local/pi/transport/tests/shutdown.py',
]:
    compile(Path(filename).read_text(), filename, 'exec')
PY
```

Document formatting command:

```sh
node_modules/.bin/oxfmt --check \
  docs/README.md docs/architecture/README.md docs/decisions/README.md \
  docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/decisions/providers/adr-219-pi-slash-command-discovery.md \
  docs/decisions/providers/adr-220-pi-extension-notifications.md \
  docs/operations/pi.md docs/reports/README.md \
  docs/reports/providers/pi-commands-notifications-2026-11-07.md
```

## Limits

Runtime checks cover Linux, offline fixtures and installed Pi 1.5.0 with temporary
resources and loopback model services. Other operating systems, real model services,
other Pi versions and browser or mobile interactions were not exercised.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, measure the
Pi adapter and related provider services with `cargo llvm-cov`, recording the source
revision, covered/total lines, a comparable baseline when available and a shared artifact.
