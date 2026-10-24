# Pi mode removal validation

Date: 2026-10-24. Environment: Linux x86_64, Rust 1.98.1, Node.js 26.8.1,
installed Pi 1.2.0, and Cargo's default features. Rust, typecheck, and lint results
were recorded at `38850143`; client and manifest results at `4261a40f`.

## Changes

Pi no longer exposes the synthetic **Native Tools** mode. The Rust adapter returns empty mode
lists from both static session settings and dynamic discovery. New sessions report no runtime
mode, and imported native histories no longer persist a mode. The shared TypeScript manifest
likewise has an empty mode list and a `null` default mode, so clients do not display a Pi mode
selector or classify Pi as having an unattended mode.

Existing Agent records may contain the former `default` mode. The adapter continues to accept
that value only as a no-op compatibility input so those sessions can resume; it is never
advertised or emitted. Other mode values remain invalid. Pi still owns its tools, extensions,
settings, trust handling, and native sessions, so tool execution behavior is unchanged.

The boundary change is recorded in
[ADR-189](../../decisions/providers/adr-189-pi-without-host-mode.md), with the operations and
architecture guides updated to match.

## Validation

- `cargo test --locked -p provider --lib local::pi -- --format=terse`: 125 passed,
  0 failed, 1 ignored, 1,079 filtered out.
- `cargo test --locked -p provider local::pi::tests::installed --lib -- --ignored`:
  1 passed against installed Pi 1.2.0, 0 failed, 1,204 filtered out.
- `cargo test --locked -p provider --lib service::agent_manager -- --format=terse`:
  102 passed, 0 failed, 1,103 filtered out.
- `cargo test --locked -p provider --lib service::agent_execution -- --format=terse`:
  99 passed, 0 failed, 1,106 filtered out.
- `npm run test --workspace=@ait/protocol -- src/provider-manifest`: 4 files passed;
  5 tests passed, 0 failed.
- The related client command below: 8 files passed; 153 tests passed, 0 failed.
- `npm run typecheck --workspace=@ait/protocol`: passed without diagnostics.
- `npm run typecheck --workspace=@ait/mobile`: passed without diagnostics.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed without warnings.
- `cargo build --locked --workspace`: passed without warnings.
- `cargo fmt --all --check`: passed.
- `node_modules/.bin/oxlint packages/protocol/src/provider-manifest.ts packages/protocol/src/provider-manifest.pi.test.ts`:
  0 warnings, 0 errors.
- Changed-file formatting command below: passed.
- `npm run check:docs`: 1 checker test passed; local links verified in 453 Markdown documents.
- `git diff --check` and `git diff --cached --check`: passed.

Related client command:

```sh
npm run test --workspace=@ait/mobile -- \
  src/provider-selection/provider-selection.test.ts \
  src/provider-selection/resolve-agent-form.test.ts \
  src/composer/agent-controls/mode.test.ts \
  src/utils/provider-definitions.test.ts \
  src/agent-profiles/internal/profile-form-model.test.ts \
  src/agent-controls/icons.test.ts \
  src/command-center/agent-control-contributions.test.ts \
  src/components/model-browser-view.test.ts
```

Changed-file formatting command:

```sh
node_modules/.bin/oxfmt --check \
  packages/protocol/src/provider-manifest.ts \
  packages/protocol/src/provider-manifest.pi.test.ts \
  docs/README.md docs/architecture/README.md \
  docs/decisions/providers/adr-156-pi-native-rpc-provider.md \
  docs/decisions/providers/adr-189-pi-without-host-mode.md \
  docs/operations/pi.md docs/reports/README.md \
  docs/reports/providers/pi-mode-removal-2026-10-24.md
```

## Limits

No full Rust workspace test suite, browser acceptance suite, native mobile build, or real model
service was exercised. Installed Pi validation used a local model fixture.

## Test coverage

Coverage was **not measured** because it was not requested. If requested, measure the affected Pi adapter
tests with `cargo llvm-cov` and report the revision, features, covered/total lines, baseline,
exclusions, and a shared artifact.
