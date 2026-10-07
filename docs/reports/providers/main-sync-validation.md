# Remaining PR synchronization after #206, #209 and #212

Updated on 2026-10-07 against main `5a00388882ddcf450d479ab67015db1418c66b24`. Retained PR heads before this update: #204 `e8c49516`, #205 `f82e5746`, #208 `16766bc8`.

## Changes

- #204 merges current main without a source conflict; local transport recovery remains scoped to its original two source/test files.
- #205 preserves both appended tests: model-change deferred notices and upload acknowledgements. The SDK production merge retains model notices and #212 upload serialization/deadlines.
- #208 preserves both document indexes and the automatic DSH transport merge: the shared desktop launcher and 64 MiB outgoing frame bound are both retained.
- No original fix or old branch history was discarded. No PR was merged into main by this update.

## Validation

#204 current branch: 11 desktop local-transport tests passed, desktop typecheck passed. #205 current branch: 143 SDK daemon-client tests and 19 mobile model-selection/upload regressions passed, mobile typecheck passed. Both branches pass `npm run build:ui-deps`, changed-file oxfmt/oxlint and documentation link checks. TypeScript line coverage was not measured.

The merged #208 Rust files under bins/ and crates/ were compared byte-for-byte with the integration source used for checks; all match. Full workspace rerun: **1960 passed, 0 failed, 15 ignored**. Workspace build, Clippy `-D warnings` and fmt passed. These Rust results apply to the matching #208/main sources, while frontend checks ran independently on the refreshed #204/#205 branches.

The first high-concurrency ordinary run failed `failed_queued_admission_does_not_block_an_independent_agent` (Closed versus Error); its focused rerun passed. The first high-concurrency coverage run failed the filesystem SSH clone URL fixture (SearchFailed) and DSH separate-permission-catalog fixture (Unavailable). Those failed runs are not reported as successes. The final full suite and coverage run used four test threads and passed without deleting, changing, or additionally skipping tests. These observations indicate timing-sensitive tests; they do not establish that the underlying flakiness has been fixed.

## Test coverage

| Scope | Covered / total lines | Line coverage |
| --- | --- | --- |
| Workspace | 55526 / 59015 | 94.09% |
| Provider | 26879 / 28852 | 93.16% |

[Current per-file coverage and exact Rust source fingerprints](main-sync-coverage.json). Linux, default features, no additional source exclusions, 15 configured ignored tests. Coverage reflects the final successful run only: raw profiles from the failed attempt were removed before it. No comparable baseline delta is claimed. Full HTML generated at `target/followup/llvm-cov/html/index.html` and retained locally; the JSON is the shared reviewable artifact.

From `/tmp/ait-integration-checks`, with `CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target/followup`, `XDG_CONFIG_HOME=/tmp/ait-test-xdg`, `LLVM_COV=/usr/bin/llvm-cov`, `LLVM_PROFDATA=/usr/bin/llvm-profdata`:

```sh
cargo test --workspace --offline -- --test-threads=4
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo build --workspace --offline
cargo fmt --all --check
cargo llvm-cov clean --workspace --profraw-only
cargo llvm-cov --workspace --html --offline --no-clean --no-fail-fast -- --test-threads=4
cargo llvm-cov report --workspace --json --output-path /tmp/ait-refresh-coverage.json
```

The cleanup command warns that --workspace is redundant with --profraw-only; only raw profiles need removing. The isolated XDG Git config prevents the user's global hooksPath from disabling fixture hooks; the user's configuration was not modified.

Known acceptance gaps remain: real GUI/system opener/PDF/drop flows, macOS/Windows, actual Codex tools/additional_tools declarations, native model service requests and user-history desktop recovery. Passing fixtures and coverage do not replace those checks. #205 remains a selected-model display/deferred-notice fix, not a mid-turn model-switch implementation.
