# Desktop listener restart and native import diagnostics

The local desktop Restart daemon action now relaunches its owned process using saved
listener settings and registers the new connection address. Fields still save on blur
or Enter. A muted hint below Listen address explains that restarting applies changes.
Remote hosts retain the existing service restart RPC.

Rebased onto upstream main `0b6bc2f1004e041e7383520ca75f3f45379e9d1e`, retaining its
removal of the host pairing section. The Rust source fingerprint remained unchanged;
mobile typecheck, host-page lint and the related frontend tests were rerun after resolving
the overlapping import block.

Native session imports now distinguish an unavailable working directory from invalid
input. The UI identifies the original directory and allows retry after restoration;
validation does not recreate directories or modify native history.

## Validation

- `nix develop --command cargo test --workspace -- --test-threads=1`: 1998 passed,
  0 failed, 15 ignored. The existing queued-checkout test exceeded its 10-second
  deadline in concurrent runs; its isolated run and final serial workspace run passed.
- `nix develop --command cargo build --workspace`, workspace Clippy with
  `--all-targets -- -D warnings`, and `cargo fmt --all --check` passed.
- Mobile and desktop typechecks, desktop main build, changed-file Oxfmt/Oxlint,
  `git diff --check`, and `npm run check:docs` passed.
- Six directly related mobile test files: 158 passed, covering import failures/retry,
  listener blur saves, restart ownership, replacement identity, reconnection and translations.
- `node apps/desktop/e2e/server-listen.e2e.mjs` passed with an Electron Expo server
  on port 8082 and the local debug daemon. Three isolated launches verified that blur
  saves without changing the running listener, Restart applies wildcard/fixed ports,
  and the host reconnects with stable identity. The final screenshot was inspected.
- Real DSH read-only import checks passed for nine sessions whose working directories
  exist. Sessions referencing deleted directories now receive actionable diagnostics.
  No model prompt was sent, and no native history was changed.

## Test coverage

Measured on macOS aarch64 with Rust 1.98.1, revision
`bea6e55b3b06e3147484896c800c1f588717a485` plus this commit's listener/import changes.
The [JSON artifact](desktop-listen-import-2026-10-08-coverage.json) records a Rust source
fingerprint, exact commands, per-file and per-crate counts, and ignored tests.

Command: `nix develop --command cargo llvm-cov --workspace --html -- --test-threads=1`.
Scope: all Rust workspace packages, default features and default coverage filters,
no additional file exclusions. Instrumented tests: 1997 passed, 0 failed, 15 ignored;
the ordinary test run additionally includes one doctest. Ignored native-provider tests
require installed CLIs, existing sessions or credentials. Linux/Windows and TypeScript
coverage were not measured. No comparable pre-change measurement exists for this revision.

| Scope | Covered / total lines | Line coverage |
| --- | ---: | ---: |
| Workspace | 55838 / 59314 | 94.14% |
| provider | 26811 / 28784 | 93.15% |
| model | 1679 / 1735 | 96.77% |
| daemon | 881 / 919 | 95.87% |

HTML was generated at `target/llvm-cov/html/index.html`; runtime profiles and HTML are
not committed. The JSON is the shared review artifact. Remaining gaps include native
provider/environment failures and platform-specific error paths; future changes to
those paths should add controlled failure cases and platform CI coverage.
