# Consolidated PR validation — 2026-10-07

This retry supersedes the environment-blocked full-suite status in the six earlier consolidation reports. Published code revisions are listed in [the coverage and source-fingerprint artifact](consolidated-qa-coverage.json). Later commits only add this report.

PR #216 is incorporated in #206; #210/#211/#213/#215/#217 are incorporated in #208. #204/#205/#209/#212 remain separate. Old PR branches and commit history are retained; no PR was merged into main.

## Source and execution

Measured source: integration baseline `c76c7dcc` plus the recorded follow-ups. The six published branches were combined locally and all files under apps/bins/crates/packages matched the tested source byte-for-byte except main's unrelated apps/mobile/eas.json release change. Exact Rust file fingerprints are in the JSON artifact. These are combined-source results, not independent per-PR coverage.

`cargo test --workspace --offline`: **1960 passed, 0 failed, 15 ignored**. The ignored installed-provider tests were not silently enabled or counted as passed. Workspace build, Clippy with `-D warnings`, and fmt checks also passed.

The first unrestricted run exposed two test failures caused by the machine's global `core.hooksPath=.githooks`. The application strips GIT_* variables before spawning Git, so GIT_CONFIG_GLOBAL alone did not isolate it. Retesting with a temporary XDG_CONFIG_HOME containing only a test Git identity restored fixture hooks: all 16 reset_workspace tests and the full workspace suite passed. The user's global configuration was unchanged; no tests were deleted or skipped to obtain this result.

Previously recorded focused frontend checks: 41 mobile, 3 SDK upload, and 5 desktop opener tests passed. Real GUI acceptance is still pending.

## Test coverage

| Scope | Covered/total lines | Line coverage |
| --- | --- | --- |
| Workspace | 55529/59015 | 94.09% |
| crates/provider | 26883/28852 | 93.18% |
| crates/api | 2095/2253 | 92.99% |
| crates/filesystem | 11561/12187 | 94.86% |
| crates/terminal | 1528/1584 | 96.46% |

Linux x86_64, default features, default cargo-llvm-cov source filters, no additional file exclusions. No comparable same-scope baseline or coverage delta is claimed. TypeScript line coverage was not measured. The checked-in JSON is the shared reviewable per-file summary and source manifest; complete HTML was generated locally under target/followup/llvm-cov/html/index.html.

Exact commands, from the integration source directory:

```sh
XDG_CONFIG_HOME=/tmp/ait-test-xdg CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target/followup cargo test --workspace --offline
XDG_CONFIG_HOME=/tmp/ait-test-xdg CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target/followup LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo llvm-cov --workspace --html --offline --no-fail-fast
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target/followup LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo llvm-cov report --workspace --json --output-path /tmp/ait-push-final-coverage.json
```

The earlier sandbox's native dependency download and local-listen restrictions were removed for this retry. The first failed coverage attempt is not presented as passing; the final HTML run completed successfully with the isolated Git configuration.

Remaining acceptance gaps: actual Codex auxiliary tools/additional_tools declarations, real OpenCode V1 model calls, DSH native desktop recovery on the user's history, GUI PDF/system chooser/folder drop, complete desktop exit/reopen, and macOS/Windows execution. Fixture coverage is not proof of those behaviors. Historical user QA results are not counted as this retry's tests.
