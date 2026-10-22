# Pi reasoning controls and runtime facts validation

Date: 2026-10-23. Validated the working tree based on
`1ae894b88d1e5e4d37c5bbeb44d5455778f11faf`, with Cargo default features and normal test
parallelism. Source hashes below identify the tested implementation before documentation
was committed.

## Finding and scope

The Pi adapter previously rejected host thinking selections and advertised no reasoning
choices. Guessing a level in forms would override native settings, while displaying only
the saved request would hide native clamping and extension changes.

The implementation follows [ADR-184](../../decisions/providers/adr-184-pi-reasoning-controls.md).
It discovers model-specific choices and native defaults, applies explicit selections through
Pi RPC, preserves effective thinking during model-only updates, and publishes native levels
separately from requested configuration. Pi form defaults preserve an unspecified selection
when no concrete default is advertised. Ait-managed child-agent facts display effective
native thinking while composer controls retain the requested selection/catalog default.

## Changes

- Read the native catalog and state in the same ephemeral process. Fail discovery when the
  required state read fails instead of silently changing defaults persisted by forms.
- Derive reasoning options from `reasoning` and `thinkingLevelMap`, including disabled,
  extended, and future levels. Validate bounded opaque IDs and let Pi resolve native clamping.
- Apply explicit model and thinking selections in order, read back native state, and preserve
  unspecified defaults on creation/resume. Clear explicit thinking to Pi's `medium` baseline.
- Apply only changed fields after startup. Preserve extension-selected or clamped thinking
  across model-only updates; do not replay unchanged host thinking on later prompts.
- Publish effective levels through `runtimeInfo.extra.thinkingLevel` and runtime-info events.
  Keep directory filter identity tied to the latest saved request, including pending updates.
- Preserve Pi's unknown form defaults while retaining other providers' first-option fallback.
- Display effective native levels in child-agent facts and compact sheets, with explicit
  handling of missing, malformed, future, and other-provider metadata.
- Update the Pi operations guide and replace stale unsupported-thinking statements in
  ADR-156 and ADR-174 with references to ADR-184.

## Validation

Environment: Linux x86_64, Rust/Cargo `1.98.1`, Node.js `26.8.1`, npm `12.0.2`,
Vitest `4.1.7`, and installed Pi `/usr/bin/pi` version `1.2.0`.
Installed-Pi tests use temporary native settings, fixture credentials, and isolated loopback
model servers. No user provider credentials or external model service were used.

### Rust tests

**154 distinct Rust tests passed.** Four opt-in tests ignored by the ordinary adapter run
were enabled separately; counts below do not double-count any successful test.

| Command                                                                                                                                   | Passed | Ignored | Test execution time |
| ----------------------------------------------------------------------------------------------------------------------------------------- | -----: | ------: | ------------------: |
| `cargo test --locked -p provider --lib local::pi`                                                                                         |    149 |       4 |              7.74 s |
| `cargo test --locked -p provider --lib service::agent_runtime::tests::thinking_filters_ignore_native_levels_in_provider_runtime_metadata` |      1 |       0 |              0.01 s |
| `cargo test --locked -p provider --lib local::pi::tests::installed -- --ignored`                                                          |      4 |       0 |              2.09 s |

The ordinary Pi run filtered out 1,057 unrelated tests; the directory regression filtered
out 1,209; the installed-Pi run filtered out 1,206. Execution times exclude Cargo build time.

Fixture tests cover slow, rejected, timed-out, exited, and malformed discovery state reads;
missing model defaults; catalog-only selection validation; future and invalid IDs; native
clamping across updates, restart, and import; clearing overrides; model/extension changes;
idle/admission/turn events; optional metadata; and retained host filter identity.
Existing Pi history, streaming, images, tools, compaction, cancellation, identity, and
process-cleanup tests also pass within the adapter filter.

Installed-Pi tests verify native defaults, model-dependent options, clamping, extension
notifications, model-only preservation, resume, and outgoing `reasoning_effort` for low,
high, xhigh, and off. They retain the existing loopback streaming/tool/image/import tests.

### UI tests

The final directly related run passed **379 tests across 38 files, with 0 failed and
0 skipped**, in **16.91 seconds** as reported by Vitest. It covers profiles, provider
selection, composer/draft behavior, and the child-agent facts line:

```sh
npm run test --workspace=@ait/mobile -- --project unit src/agent-profiles src/composer src/provider-selection src/panels/agent-facts-line.test.ts
```

Tests verify Pi's unspecified defaults, model/option default markers, other providers'
fallbacks, draft persistence, requested composer selections, effective native facts,
extension snapshot updates, and missing/malformed metadata.

### Build and static checks

- `cargo build --locked --workspace` passed without warnings.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
- `cargo fmt --all --check` passed.
- `npm run build:sdk` passed, rebuilding local protocol/client definitions.
- `npm run typecheck --workspace=@ait/mobile` passed after the runtime-display additions.
- Changed-file `oxlint --deny-warnings` passed for all 9 changed TypeScript/TSX files
  with zero warnings or errors; changed-file `oxfmt --check` passed.
- Python fixture syntax passed `ast.parse`.
- `npm run check:docs` passed the documentation checker tests and local link checks.
- Changed-document formatting and staged/unstaged whitespace checks passed.

## Limits

- Installed Pi `1.2.0` is the tested native version; newer versions were not exercised.
- Model requests use a local fixture service. Authenticated external model behavior,
  provider billing, and real model output were not validated.
- UI tests exercise selection/form models and the facts-line hook with mocked snapshots;
  no manual browser acceptance, native Android/iOS build, physical device, or macOS/Windows
  validation was run.
- The effective facts line is rendered in existing Ait-managed child-agent views. Regular
  agent tabs retain composer controls. Pi-native extension subagents are not integrated.
- Missing native defaults remain host-dependent; clearing a prior explicit selection
  requests `medium`, which Pi can clamp, rather than invoking an unset operation.

## Test coverage

Coverage was **not measured** because it was not requested. Passing test counts report
execution results, not line or branch coverage. No percentage, instrumented artifact, or
comparison with a measured baseline is claimed.

Only Pi adapter behavior, the affected directory regression, and directly related UI tests
were executed. No full Rust workspace suite or workspace coverage run was performed.
If coverage is requested, measure the affected provider paths with focused `cargo llvm-cov`
tests and publish the exact source revision, default-feature scope, exclusions, counts,
and a reviewable artifact.

## Merge Danger

**Door:** two-way, with configuration compatibility considerations on rollback.

There is no storage migration or protocol schema change. Revert restores the previous
adapter and UI behavior, but the old Pi adapter rejects nonempty thinking selections.
Agents/profiles saved with explicit Pi reasoning choices may need those overrides cleared
when returning to the older implementation.

**Blast Radius:** Pi discovery, create/resume/configuration, runtime metadata, and Pi form defaults.

Other providers share default normalization and display helpers; regressions verify that
their first-option fallback and requested-selection behavior remain intact. Provider-owned
tests stay in `crates/provider`; no daemon test relocation or new dependency boundary is added.

## Source snapshot

SHA-256 values below identify all changed Rust, Python, and TypeScript/TSX source/test files.
Documentation edits do not change this application source snapshot.

```text
83945753c8bd37562d366b3eaaf4179890c42f018d611d860777f4a9434e054d  apps/mobile/src/agent-profiles/internal/profile-form-model.test.ts
682ad3b6893b08e4ed67b46814ab316f1cd036569a88c6a6015c1ea6941e08dc  apps/mobile/src/agent-profiles/internal/profile-form-model.ts
b194d9a170f83d97c1f8f911888dd799d79c70ae480e7fc0e827270144427dc0  apps/mobile/src/composer/agent-controls/utils.test.ts
8392efd9521ab24f3a31983fbc0bf849e2f91e35e850e70f9db64a035ce0b6ea  apps/mobile/src/composer/agent-controls/utils.ts
b6279d9db828395b2bdf702a2f855c30471d0dfc82f5706219dfaa5b34d1e2f2  apps/mobile/src/composer/draft/input-draft.test.ts
f8c518e2d98a259697abc790179176259437c7183cb3293cca1fdb74cbf7fb12  apps/mobile/src/panels/agent-facts-line.test.ts
588e20e7dfdff45d3957efce0685673aa38cd5914adb4dad0f0a332ffc587489  apps/mobile/src/panels/agent-facts-line.ts
e788c457adcfbc35c08ed220d4ea433e96e5ccb52f07aeb22841ef62c040b02d  apps/mobile/src/provider-selection/resolve-agent-form.test.ts
04af089f3cab68826c65b053f99706b08c646fc315ebc2a7fdf1637c7688947d  apps/mobile/src/provider-selection/resolve-agent-form.ts
d37f0f5b49ad61fad247cddd57b18ffae837173d7780b1af34b3181d7be29754  crates/provider/src/local/pi.rs
18f60317e1b2ce9bd732b8ceaa12065f502d2abdf1193f505dd6079c4da929bf  crates/provider/src/local/pi/config.rs
350d0f9b1de68b40794e8bb9bd6cf2272896840eaff080e733589cee84250331  crates/provider/src/local/pi/session.rs
c0b50fc54b42a7232b0e74ca1ebb4c459409c9b052f6e7d28d10925325767fc4  crates/provider/src/local/pi/tests.rs
2a50c7eba9b2b61bd21cc1b6210eb1783fdafd5fd8c171b93c26cfacb8ed83b2  crates/provider/src/local/pi/tests/discovery.rs
e7c5a331a65ea6298f85926cf73b8f80d17e9b927fdfaf89b3fdcf779e799e98  crates/provider/src/local/pi/tests/fixtures/pi.py
e03ff621df20136a6f12867636c7bf663dae2bb9b770b65c605dc0f901894fbd  crates/provider/src/local/pi/tests/installed.rs
d8114c73a69e7fe4346e2b5e8d65c12d648a94a2fd96a46f3cbff698045d1948  crates/provider/src/local/pi/tests/installed/reasoning.rs
4a731b2b0e9d4996f875c59c8188388380d32dafa2523d74eb7ccbf5271e7260  crates/provider/src/local/pi/tests/reasoning.rs
fc0b49c5b5d7757e8805e7184e4df64eff07a11d4096ac643dcbd82003069cc4  crates/provider/src/local/pi/tests/review.rs
c9fdaf94850a9c03a275a507ad3694903cb8b057a2c80030517df8801a9e8ab0  crates/provider/src/service/agent_runtime/tests.rs
```
