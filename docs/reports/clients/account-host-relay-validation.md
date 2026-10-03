# Account host relay validation

The PR uses `apps/desktop`, `apps/mobile`, `bins/daemon`, and the `relay` crate.
It adds desktop email/password sign-in, automatic host registration and discovery,
explicit host selection, reverse relay connections, and independent downloads. The API
base is `https://dash.ait-app.com:8443/api`. UI labels, errors, and new usage documentation
are in English. Login actions and the shared registration/control retry delay are
factored into named helpers.

Integration with main preserves the server version, background Git fetch composition,
and existing capability flags. Catalog tests distinguish the `connection.single.v1`
protocol marker from RPC methods. The dependency guard permits `api` to own the
relay adapter and prevents `relay` from depending on other workspace crates.
See [ADR-074](../../decisions/clients/adr-074-account-host-relay.md).

## Validation

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo build --workspace --offline`: passed.
- `cargo test --workspace --offline`: 1,578 passed, 0 failed, 3 ignored.
- App login and Rust transport tests: 58 passed, including the real-server integration test.
- Desktop account, daemon manager, and Rust lifecycle tests: 19 passed, including
  startup, restart, saved listeners, runtime identity, and process ownership checks.
- App and desktop TypeScript checks passed. ESLint for the changed app files and
  Oxlint for the account/runtime desktop files completed without warnings.
- Oxfmt checks passed for all 22 TypeScript files changed by the PR; `git diff --check` passed.
- No added line in the PR contains Chinese text. Existing unrelated translations are unchanged.

The real-server tests used the freshly built `target/debug/daemon` through
`AIT_TEST_RUST_SERVER` (app) and `AIT_SERVER_BIN` (desktop). The three ignored Rust tests
require a locally installed/authenticated Claude or Codex CLI; two perform real inference.
No production deployment or account mutation is part of these checks.

## Test coverage

Measured on Linux x86_64 with Rust 1.98.1 and cargo-llvm-cov 0.9.1, using default
features and source filtering. The measured source tree is identified by hashes in
the committed [coverage artifact](account-host-relay-coverage.json), based on main
`6ce645040c0ef03d9d677ce9224c0fa9cdcb028b`. Documentation edits do not change that tree.

Commands:

```sh
cargo llvm-cov --workspace --html --locked --offline
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-pr138-current-summary.json
cargo llvm-cov report --lcov --output-path /tmp/ait-pr138-current.lcov
```

The instrumented run independently passed 1,578 tests with 3 ignored; it is not added
to the ordinary test count. Doctests are not instrumented. macOS and Windows were not
tested, and no comparable Linux baseline was measured, so no coverage delta is claimed.

| Scope | Line coverage | Covered / total lines |
| --- | ---: | ---: |
| Rust workspace | 91.76% | 44,517 / 48,513 |
| api | 92.41% | 1,864 / 2,017 |
| daemon | 94.31% | 879 / 932 |
| model | 94.82% | 696 / 734 |
| protocol | 100.00% | 73 / 73 |
| relay | 60.83% | 278 / 457 |

The HTML report was generated at `target/llvm-cov/html/index.html`. The committed JSON
is the shared review artifact, including per-crate statistics, source hashes, and
uncovered lines in changed production files.

Remaining gaps: the relay download path has 0 / 134 covered lines in this automated
workspace run. The single-connection scheduler covers 150 / 205 lines, and the relay
bridge covers 53 / 67 lines. Follow-up coverage should exercise download transfer and
failure/cancellation paths, plus scheduler saturation and shutdown races. Earlier
manual end-to-end checks are not counted as coverage for this revision.
