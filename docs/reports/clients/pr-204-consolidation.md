# PR #204: consolidation and QA follow-up (2026-10-07)

## Main synchronization after #206, #209 and #212 merged

Updated against main `5a003888` on 2026-10-07. This is the latest validation entry; the earlier combined-source report below is historical. The source is the previous PR head plus this main merge. No production fix was dropped or rewritten.

Desktop local-transport: 11 passed; desktop TypeScript passed. `npm run build:ui-deps`, changed-file oxfmt and oxlint passed. #205 retains both the upload acknowledgement test from #212 and the deferred-model notice test; the conflict was overlapping appended test blocks.

### Test coverage

TypeScript line coverage was not measured; this synchronization uses focused behavior tests. No new Rust behavior is introduced by this PR, so independent Rust tests are not required for this frontend update. The Rust-bearing #208 synchronization is validated separately. GUI/native-provider acceptance remains outside these tests.

Base: upstream `d6a5d246afae820a41cb90f3a9c40adbc50f8a9b`. Group: recovery.
This supersedes older validation claims for the updated code. Original reports are historical.
All six prepared branches were merged locally; every file under apps/bins/crates/packages matches the tested integration working tree byte-for-byte, except upstream's unrelated apps/mobile/eas.json release change. Exact changed-file fingerprints are in the adjacent JSON artifact.

## Validation

Tested source: integration baseline `c76c7dcc` plus archive, desktop DSH discovery, and QA follow-ups. Default features, Linux. The prepared group is independently scoped; checks below ran on the combined source, not each PR in isolation.

- `cargo test -p provider metadata --offline -- --skip local::deepseek_harness::native::tests`: 37 passed, 2 ignored; native TCP Host tests excluded from this focused run.
- `cargo test -p provider native::projection --offline`: 2 passed.
- Prior archive checks: API workspace_archive 2 passed; terminal archive_cleanup 2 passed; provider directory_only_hosts_check 1 passed.
- Prior DSH checks: 26 passed / 2 ignored with native TCP Host tests excluded; installed desktop CLI launcher 1 passed separately.
- Mobile Vitest binary-preview/upload-regression/transport: 41 passed. SDK uploadFile filter: 3 passed, 140 not selected. Desktop file-opener: 5 passed. SDK build, desktop/mobile TypeScript, changed-file oxfmt/oxlint passed.
- Commit preparation: `cargo build --workspace --offline`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check` passed.
- `cargo test --workspace --offline` stopped at API: 38 passed, 48 failed. Local TCP listening is denied with EPERM in this sandbox. This is NOT a passing full workspace run.
- `cargo llvm-cov --workspace --html --offline --no-fail-fast` could not build: sherpa-onnx-sys attempted to download its native archive, and GitHub DNS is unavailable to shell commands. Full workspace HTML coverage was not generated.

## Test coverage

Current full workspace coverage: **not measured**, blocked by the native dependency download above. No comparable baseline. CI in a network-enabled runner must rerun full tests and HTML coverage before acceptance. TypeScript line coverage was not measured.

Focused provider measurement on the same implementation (default features, no extra file exclusions):

| Scope | Covered/total lines | Coverage |
| --- | --- | --- |
| metadata_model.rs | 44/44 | 100.00% |
| codex/metadata.rs | 82/90 | 91.11% |
| opencode/metadata.rs | 62/198 | 31.31% |
| deepseek_harness/native/projection.rs | 76/132 | 57.58% |
| Provider files, focused tests only | 7943/28852 | 27.53% |

Commands: `cargo llvm-cov -p provider --lib --html --offline -- metadata --skip local::deepseek_harness::native::tests`; `cargo llvm-cov -p provider --lib --no-clean --html --offline -- native::projection`; `cargo llvm-cov report -p provider --json`. LLVM_COV=/usr/bin/llvm-cov, LLVM_PROFDATA=/usr/bin/llvm-profdata. The percentages are focused coverage, not full provider/workspace coverage; they apply to the combined source. The checked-in fingerprints and table are the reviewable summary; raw HTML remains local.

Remaining acceptance: real OpenCode V1 HTTP calls, Codex actual request tools/additional_tools absence, DSH native TCP recovery, GUI PDF/folder drop/system chooser, macOS/Windows, and complete desktop restart. Do not mark these passed from mocks or older reports.
