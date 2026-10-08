# Codex first-turn rewind compatibility

Codex CLI 0.161.0 no longer exposes `thread/rollback`. Rewinding the first user
message previously forked the whole thread and called that removed method, producing
`AgentIo`, including when the only turn had been interrupted.

First-turn rewinds now request an exclusive `thread/fork` with `beforeTurnId` and
read back the persisted fork. An older server that ignores this field still uses
rollback on the new fork after validating its identity and turn count. Later-turn
rewinds retain their inclusive `lastTurnId` behavior. Original native history is preserved.

## Validation

- `nix develop --command cargo test -p provider rewind -- --test-threads=1`:
  10 passed. The new regression covers an interrupted first turn with rollback removed,
  and a legacy server that ignores `beforeTurnId`. It checks the exclusive fork,
  source preservation, absence of model requests, and legacy rollback's target identity.
- Installed Codex 0.161.0 generated its experimental schema locally: `beforeTurnId`
  is supported, while `thread/rollback` is absent.
- A real CLI check copied only the affected rollout and its paginated history rows into
  a temporary `CODEX_HOME`, without credentials. Exclusive fork and persisted read
  returned zero turns. The original rollout hash was unchanged; temporary data was removed.
  No model request was sent. Personal session contents and runtime databases are not committed.
- Workspace build, workspace Clippy with `--all-targets -- -D warnings`, Rust format
  check and Python fixture syntax validation passed.

## Remote workspace archive diagnosis

The reported remote host was actually running daemon 0.0.22. That version immediately
rejects workspace archive steps when the shared job permit is occupied. Commit
`a187af780ef64657c68bfc70ddc3d685fd66a722`, included in 0.0.23-beta.1 and current dev,
already queues these steps and cancels waiting safely during shutdown.
`nix develop --command cargo test -p api archive_waits_for_shared_capacity_before_mutating_metadata`
passed. The remote installation needs updating to receive that existing fix; this diagnosis
did not archive user workspaces or restart the remote daemon.

## Test coverage

Measured on macOS aarch64, Rust 1.98.1, revision `7b356d67` plus this commit's
Codex rewind patch. The [JSON artifact](codex-first-turn-rewind-2026-10-08-coverage.json)
records the source patch hash, exact commands, all crate counts and ignored tests.

`nix develop --command cargo llvm-cov --workspace --html -- --test-threads=1`
executes the full workspace unit and integration suites under instrumentation:
**1998 passed, 0 failed, 15 ignored**.
`nix develop --command cargo test --workspace --doc` additionally passed the
1 workspace doctest. These are execution counts, separate from coverage.

| Scope | Covered / total lines | Line coverage |
| --- | ---: | ---: |
| Workspace | 55847 / 59325 | 94.1374% |
| provider | 26821 / 28795 | 93.1446% |
| Codex rewind | 108 / 113 | 95.5752% |

Same-scope baseline: [listener/import validation](../clients/desktop-listen-import-2026-10-08.md),
94.1397% (55838/59314); change **-0.0023 percentage points**.
Default features and coverage file filters were used, with no extra exclusions.
The 15 existing ignored native-provider tests require installed CLIs, existing sessions
or authentication. Linux/Windows and TypeScript coverage were not measured.
HTML was generated at `target/llvm-cov/html/index.html`; the JSON is the shared artifact.
Remaining gaps include invalid native responses and platform/CLI failures; add controlled
failure cases when changing those paths and validate native compatibility on other platforms.
