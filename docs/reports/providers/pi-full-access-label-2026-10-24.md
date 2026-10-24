# Pi Full Access mode label validation

Date: 2026-10-24. Validated working-tree changes based on
`c2181d69572db746dc6526a5de7457d724cf9153` on Linux x86_64, with Rust 1.98.1,
Node.js 26.8.1, and Cargo's default features.

## Changes

Pi's single mode is displayed as **Full Access** in the Rust catalog and shared
TypeScript manifest. Its description clarifies the existing approval behavior and
explains that cancelling interactive extension dialogs can prevent extensions that
require input from running. The mode ID remains `default`; execution behavior,
native configuration, trust handling, and extension responses are unchanged.

The Pi operations guide and [ADR-156](../../decisions/providers/adr-156-pi-native-rpc-provider.md)
use the same label. The shared manifest test expects **Full Access**.

## Validation

| Command                                                                                                                                                                                                                                                                                           | Result                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `cargo test --locked -p provider --lib local::pi`                                                                                                                                                                                                                                                 | 125 passed, 0 failed, 1 ignored, 1,072 filtered out. The ignored test requires installed Pi and explicit opt-in. |
| `npm run test --workspace=@ait/protocol -- src/provider-manifest`                                                                                                                                                                                                                                 | 4 test files passed; 5 tests passed, 0 failed.                                                                   |
| `npm run typecheck --workspace=@ait/protocol`                                                                                                                                                                                                                                                     | Passed without diagnostics.                                                                                      |
| `cargo clippy --locked --workspace --all-targets -- -D warnings`                                                                                                                                                                                                                                  | Passed without warnings.                                                                                         |
| `cargo build --locked --workspace`                                                                                                                                                                                                                                                                | Passed.                                                                                                          |
| `cargo fmt --all --check`                                                                                                                                                                                                                                                                         | Passed.                                                                                                          |
| `node_modules/.bin/oxlint packages/protocol/src/provider-manifest.ts packages/protocol/src/provider-manifest.pi.test.ts`                                                                                                                                                                          | 0 warnings, 0 errors.                                                                                            |
| `node_modules/.bin/oxfmt --check packages/protocol/src/provider-manifest.ts packages/protocol/src/provider-manifest.pi.test.ts docs/operations/pi.md docs/decisions/providers/adr-156-pi-native-rpc-provider.md docs/reports/README.md docs/reports/providers/pi-full-access-label-2026-10-24.md` | Passed.                                                                                                          |
| `npm run check:docs`                                                                                                                                                                                                                                                                              | Passed.                                                                                                          |
| `git diff --check`                                                                                                                                                                                                                                                                                | Passed.                                                                                                          |

## Test coverage

Coverage was **not measured** because it was not requested. The counts above are
test execution results. If requested, measure the affected Pi adapter tests with
`cargo llvm-cov` and report the revision, features, covered/total lines, baseline,
exclusions, and a shared artifact.
