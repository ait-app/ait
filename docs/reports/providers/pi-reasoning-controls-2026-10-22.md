# Pi reasoning controls validation

Date: 2026-10-22. Validated the uncommitted working tree based on
`c8f89d0b642e509f8633cc6f46242718f58b3ce1`, using Cargo's default features.
Source hashes below identify the tested code.

## Environment and scope

- Linux x86_64; Cargo 1.98.1; Node.js 26.8.1; Vitest 4.1.7.
- Installed Pi: `/usr/bin/pi`, version 1.2.0.
- Primary Rust validation covers Pi discovery, controls, native events, history, resume,
  and installed-Pi integration. An earlier broader provider-crate run also exercised
  shared services and other adapters. Build and Clippy cover the workspace; the full
  Rust workspace test suite was not run.
- UI unit validation covers profiles, composer, provider selection, and the facts line.
- External model services and user provider credentials were not used.

## Provider regression tests

```sh
cargo test --locked -p provider --lib local::pi
cargo test --locked -p provider --lib
cargo test -p provider local::pi::tests::installed --lib -- --ignored
cargo test --locked -p provider --doc
```

The final Pi-only run passed: **148 passed, 0 failed, 3 ignored, 1,056 filtered out**,
in 7.82 seconds. The three ignored Pi cases were enabled separately below.

The final broad provider-crate run passed: **1,162 passed, 0 failed, 45 ignored**,
in 45.36 seconds. It includes the same **148 passing Pi tests**. Ignored cases require explicit opt-in for
installed programs or external accounts; the three Pi cases were enabled separately.

Installed Pi validation passed: **3 passed, 0 failed**, in 2.08 seconds. The tests use
isolated temporary settings and a loopback model server. They verify native discovery
defaults, model-dependent choices, clamping, extension notifications, resume, and outgoing
`reasoning_effort` values for low, high, xhigh, and off. They also retain the existing
streaming, tools, images, import, and resume integration coverage. Pi 1.2.0 is the tested
version; this does not establish compatibility with every later version.

Provider doctests passed with **0 tests discovered**.

Discovery regressions cover slow, rejected, timed-out, exited, and malformed state reads,
missing native model state, and selection validation that reads only the model catalog.
Reasoning regressions cover future IDs, missing and malformed optional metadata, native
clamping across updates/restart/import, clearing explicit selections, extension changes
while idle and during admission/turns, and directory filtering by the stored request.

## UI unit tests and static validation

```sh
npm run test --workspace=@ait/mobile -- --project unit src/agent-profiles src/composer src/provider-selection src/panels/agent-facts-line.test.ts
npm run build:sdk
npm run typecheck --workspace=@ait/mobile
cargo build --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm run check:ui
npm run check:docs
git diff --check
```

UI tests passed: **367 tests across 38 files**, in 30.11 seconds. They cover Pi's unknown
defaults, concrete defaults from option metadata, profile/draft persistence, requested
selection labels and checkmarks, effective native facts, and other providers' fallback
behavior.

The final "Default" label check passed: **41 tests across 2 files**, in 0.793 seconds.
These tests are a focused rerun of cases included in the broader UI suite, verifying the
shortened label and unchanged handling of unspecified reasoning selections.

```sh
npm run test --workspace=@ait/mobile -- --project unit src/composer/agent-controls/utils.test.ts src/composer/draft/input-draft.test.ts
```

SDK build, mobile typechecking, workspace build, Clippy, Rust formatting, changed-file
UI lint/formatting, Python fixture syntax, documentation tests/links, and diff whitespace
checks passed.

Changed TypeScript/TSX and Markdown files were checked with `node_modules/.bin/oxfmt
--check` and the explicit changed-file list. UI lint used the same TypeScript/TSX list
with `node_modules/.bin/oxlint --deny-warnings`. Python fixture syntax passed `ast.parse`.

## ADR compatibility review

- [ADR-156](../../decisions/providers/adr-156-pi-native-rpc-provider.md) now records the
  reasoning contract and explicitly replaces its original unsupported-thinking limitation.
  Its scope includes the shared UI, and the documentation index and Pi operations guide
  describe the same behavior. The effective-reasoning facts line is limited to existing
  Ait-managed child-agent rows and compact sheets. Pi-native subagent extensions are not
  integrated into those views; regular agent tabs retain the composer controls.
- [ADR-174](../../decisions/providers/adr-174-pi-session-import.md) contained a stale
  unsupported-thinking statement. It now references ADR-156; native-history ownership,
  explicit refresh, ordinary restart retention, and the rewind limitation are preserved.
- [ADR-034](../../decisions/providers/adr-034-agent-config-session-events.md) and
  [ADR-041](../../decisions/providers/adr-041-agent-controls-provider-inspection.md):
  host configuration remains separate from effective runtime facts and applies to later
  turns. ADR-156 documents Pi's adapter-specific `medium` resolution when clearing an
  override because Pi has no unset RPC. Native clamping does not overwrite the requested
  selection or its directory-filter identity.
- [ADR-089](../../decisions/providers/adr-089-provider-owned-composition.md) and
  [ADR-179](../../decisions/providers/adr-179-provider-owned-tests.md): adapter behavior
  and its tests stay in `provider`; no daemon dependency or new public port is introduced.
- [ADR-144](../../decisions/providers/adr-144-provider-error-reporting.md),
  [ADR-150](../../decisions/providers/adr-150-provider-compaction-lifecycle.md), and
  [ADR-176](../../decisions/providers/adr-176-pi-user-image-projection.md): existing
  diagnostics, compaction, and user-image contracts remain covered by the Pi regression
  suite. Discovery and new controls use the existing bounded transport.

No unresolved conflict was found in these affected decisions. There is no new domain
boundary, database migration, or RPC schema change requiring a separate boundary ADR.

## Test coverage

Line and branch coverage were **not measured**, because instrumented coverage was not
requested. Passing test counts are execution evidence, not coverage percentages. There
is no current coverage artifact or comparable measured baseline. If requested later,
measure the affected provider tests with `cargo llvm-cov`, identify the exact revision
and exclusions, and publish a reviewable report. Native Android/iOS builds, physical
devices, and authenticated external model services were not exercised.

## Source snapshot

The following SHA-256 values cover changed Rust, Python fixture, and TypeScript/TSX files.
Documentation-only edits after test execution do not change this source snapshot.

```text
83945753c8bd37562d366b3eaaf4179890c42f018d611d860777f4a9434e054d  apps/mobile/src/agent-profiles/internal/profile-form-model.test.ts
682ad3b6893b08e4ed67b46814ab316f1cd036569a88c6a6015c1ea6941e08dc  apps/mobile/src/agent-profiles/internal/profile-form-model.ts
17e685731efcce91b55ecf59be7ea1f4d672c892b2628c898806888a45868747  apps/mobile/src/composer/agent-controls/index.tsx
2ebeea3d9b8c6d1c5445302e950046e1ae70a2a8be544e798ba423f47f23d9d4  apps/mobile/src/composer/agent-controls/utils.test.ts
7c738b9eed407981d556bfc5fe624f269551db1df45af16fd621ce3a7dda05aa  apps/mobile/src/composer/agent-controls/utils.ts
0434c92c72326f1d45f9223b645a29bcaedadcafe5af7b7fc8dbcd251b25c48e  apps/mobile/src/composer/draft/input-draft.test.ts
637da74fedc9b1188d6e8c7850198e66141853332af4db9eea677b8e0e5fc8dd  apps/mobile/src/i18n/resources/ar.ts
a7f6d1548b129c28158831e73af9e3814d754a8f1f6e243f25ef5c8b83c425de  apps/mobile/src/i18n/resources/en.ts
cd8a668506a74ef21aa34310f9e917779c77d7d48c1cd9cbe2ecef6a4a1ff3e2  apps/mobile/src/i18n/resources/es.ts
5cd8defa729f05450120a6895f97faa811fdc1b0095f8432e86c64b27482c1ac  apps/mobile/src/i18n/resources/fr.ts
a844b4b7e395f62a297c3affb0bced7fe11f72edc48bb2f283fc3a73fc27466d  apps/mobile/src/i18n/resources/ja.ts
29c75a23d41ec15b1714a98609ef47eab2e9ecb03a3661dca8c6c4203cb7ac97  apps/mobile/src/i18n/resources/ko.ts
ae9f0dda83f9168fef022abeecfcee30ea22275b85e98a565c430fabb8500bd3  apps/mobile/src/i18n/resources/pt-BR.ts
17343a5f371851a6f5fb2615c637f6e19a125ecb939be8c44f934109142ecb75  apps/mobile/src/i18n/resources/ru.ts
18cf27ec58ea2646befbd5cfea396cad359dc25e3cee5748e61cebf41d8f0a23  apps/mobile/src/i18n/resources/zh-CN.ts
ac8d41ee8bf49c9f7af3ec75389ad5db320d0ce5e2b15cc06937b97a7084e070  apps/mobile/src/panels/agent-facts-line.test.ts
da44ea9b324f1c50b14f4bcf7adee7505115cd383a51ff9eac1482f2eb926ca1  apps/mobile/src/panels/agent-facts-line.ts
e788c457adcfbc35c08ed220d4ea433e96e5ccb52f07aeb22841ef62c040b02d  apps/mobile/src/provider-selection/resolve-agent-form.test.ts
04af089f3cab68826c65b053f99706b08c646fc315ebc2a7fdf1637c7688947d  apps/mobile/src/provider-selection/resolve-agent-form.ts
d37f0f5b49ad61fad247cddd57b18ffae837173d7780b1af34b3181d7be29754  crates/provider/src/local/pi.rs
18f60317e1b2ce9bd732b8ceaa12065f502d2abdf1193f505dd6079c4da929bf  crates/provider/src/local/pi/config.rs
45f85c242e25d9f889e4a653063bc7b972869ce7f95d66929efb27f6ad44c6e6  crates/provider/src/local/pi/session.rs
c0b50fc54b42a7232b0e74ca1ebb4c459409c9b052f6e7d28d10925325767fc4  crates/provider/src/local/pi/tests.rs
2a50c7eba9b2b61bd21cc1b6210eb1783fdafd5fd8c171b93c26cfacb8ed83b2  crates/provider/src/local/pi/tests/discovery.rs
b4076888eb1dce42741343e7619a696733bbfdda74ed54e15247ec0a965ffbea  crates/provider/src/local/pi/tests/fixtures/pi.py
e03ff621df20136a6f12867636c7bf663dae2bb9b770b65c605dc0f901894fbd  crates/provider/src/local/pi/tests/installed.rs
0c0672b2342b3c79ad958ce2da22efdda6abad6764350fef8d6732149fc06736  crates/provider/src/local/pi/tests/installed/reasoning.rs
5d396320bf10dee6d48bcc56768c847677683b66d4b0482053e7ad69144a0922  crates/provider/src/local/pi/tests/reasoning.rs
fc0b49c5b5d7757e8805e7184e4df64eff07a11d4096ac643dcbd82003069cc4  crates/provider/src/local/pi/tests/review.rs
c9fdaf94850a9c03a275a507ad3694903cb8b057a2c80030517df8801a9e8ab0  crates/provider/src/service/agent_runtime/tests.rs
```
