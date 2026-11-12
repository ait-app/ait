# Provider Crate Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish splitting `provider` (129.7k lines, 1,696 tests in one test binary) into a contract crate, a native toolkit crate, one crate per adapter, the runtime and a built-in assembly crate, so that Cargo enforces the adapter/runtime boundary and an adapter edit rebuilds and tests only that adapter.

**Architecture:** Adapters and the runtime both depend on `provider-core` and never on each other in production. The runtime keeps the name `provider` and its generic `Providers` container; a new `provider-builtin` crate is the only place that names concrete adapters. `provider-omp`, extracted early by [ADR-249](../decisions/providers/adr-249-omp-adapter-crate.md), is re-pointed first and proves the contract surface before any other adapter moves.

**Tech Stack:** Rust 2024 Cargo workspace, `cargo nextest`, [`scripts/verify.mjs`](../../scripts/verify.mjs), the dependency guard in [`bins/daemon/tests/dependencies.rs`](../../bins/daemon/tests/dependencies.rs).

**Spec:** None exists. The Design section below is the specification; Task 1 records it as an ADR.

**Status (2026-11-12):** Rewritten against `main` at `f3a2c3d3`; nothing below is implemented. The 2026-11-10 version predated `provider-omp` and is replaced. What changed: `provider-omp` is the pilot instead of a task; `Providers` stays in the runtime as a container; three contract files are split by item instead of dragging RPC request types into the contract crate; adapter tests use the `provider::testing` facade that ADR-249 introduced; ADR `paths` metadata and the full list of fixture consumers are covered.

## Why

Single runs on macOS arm64 (10 cores), warm incremental dev profile, after `touch` on one file:

| Edited file                                  | Command                                      | Wall time |
| -------------------------------------------- | -------------------------------------------- | --------- |
| `crates/provider/src/local/minimax.rs`       | `cargo check -p provider --lib`              | 1.4 s     |
| `crates/provider/src/local/minimax.rs`       | `cargo nextest run -p provider --no-run`     | 34.1 s    |
| `crates/provider/src/ports/agent_session.rs` | `cargo nextest run -p provider --no-run`     | 40.4 s    |
| `crates/provider-omp/src/lib.rs`             | `cargo nextest run -p provider-omp --no-run` | 4.9 s     |
| `crates/provider/src/local/minimax.rs`       | `cargo nextest run -p provider-omp --no-run` | 5.6 s     |

Type checking is already fast. The cost is the test binary: every adapter edit rebuilds and relinks 130k lines before one test can run, and `npm run verify` then selects all 1,696 tests. `provider-omp` (21k lines, 245 tests) shows what an extracted adapter costs. The last row is an edit inside `provider`: it rebuilds `provider-omp`'s tests because that adapter links the whole runtime crate today.

Of the 159 non-merge commits since 2026-10-01 that touched the provider family, 50 (31%) changed one adapter, optionally with the shared toolkit, and nothing else; 28 (18%) changed only the runtime; 74 (47%) crossed adapter and runtime or contract code. About a third of the changes get the narrow path outright. The rest still build smaller test binaries, and Codex's `webrtc`, `rtc` and `opus2` stop gating the compile of the runtime and the other adapters.

## Design

Target graph. Solid arrows are normal dependencies, dotted arrows are dev-only.

```
domain <- model <- provider-core <- provider-native <- provider-<adapter> x8
                        ^                                     ^
                        |                                     |
                     provider (runtime)  <----------  provider-builtin
                        ^      ^                              ^
                       bot    api                           daemon

dev-only:  provider-<adapter> ..> provider        (adapter tests drive the real runtime)
           provider           ..> provider-codex  (the runtime's execution suite drives the Codex fake)
           api                ..> provider-builtin
```

| Crate                                                                                                                                            | Contents (current path under `crates/provider/src`)                                                                                                                                                                                 | Lines, non-test                          | Test attributes                |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ------------------------------ |
| `provider-core`                                                                                                                                  | The contract table below                                                                                                                                                                                                            | about 1.6k                               | about 20                       |
| `provider-native`                                                                                                                                | `local/{acp_config,acp_permissions,acp_streaming,acp_transport,compaction,configuration,diagnostics,elicitation,images,metadata_process,notes,process_group,prompt_blocks,sandbox,session_preview,summary_model,tool_detail,usage}` | 4.9k                                     | 131                            |
| `provider-codex`, `provider-pi`, `provider-opencode`, `provider-deepseek-harness`, `provider-claude`, `provider-antigravity`, `provider-minimax` | `local/<adapter>.rs` and `local/<adapter>/`                                                                                                                                                                                         | 8.0k, 4.7k, 5.6k, 4.5k, 5.3k, 2.0k, 1.0k | 272, 306, 156, 113, 85, 74, 23 |
| `provider-omp`                                                                                                                                   | Already extracted; only its dependencies change                                                                                                                                                                                     | 9.8k                                     | 267                            |
| `provider` (runtime, name unchanged)                                                                                                             | `service`, `rpc`, `storage`, `dispatch`, `connection`, `capabilities`, `timeline_projection`, `skills`, `support`, `diagnostics`, `installation`, `composition`, `testing`, and the runtime-only halves of `ports` and `protocol`   | about 24k                                | about 590                      |
| `provider-builtin`                                                                                                                               | New: the built-in adapter list with its launcher defaults and environment overrides, and the tests that need several real adapters with the real runtime                                                                            | about 0.1k                               | about 6                        |

Contract table. A module moves whole when every item in it is used by an adapter or by a port signature. A module that mixes adapter-facing values with RPC request types is split by item, and each half keeps the module name in its crate.

| Module                                                                                      | To `provider-core`                              | Stays in `provider`                                                                                                        |
| ------------------------------------------------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `ports/{agent_session,account_usage,controls,diagnostics,environment,native_history}`       | whole                                           | —                                                                                                                          |
| `ports/bot_session` (created by Task 2)                                                     | whole                                           | —                                                                                                                          |
| `ports/usage_ledger`                                                                        | `UsageBoundary`, `RequestUsage` and their tests | `UsageLedger`, `NoopUsageLedger`, `CallSource`, `CallBegin`, `CallFinish`, the query and row types                         |
| `ports/{agent,mcp_scope}`                                                                   | —                                               | whole                                                                                                                      |
| `protocol/{prompt,usage,agent_config}`                                                      | whole (`ConfigPatch` carries `NullableSetting`) | —                                                                                                                          |
| `protocol/provider`                                                                         | `Details`                                       | `ListRequest`, `FeaturesRequest`, `SnapshotRequest`, `RefreshRequest`                                                      |
| `protocol/timeline`                                                                         | `NativeItem`                                    | `Cursor`, `Direction`, `Projection`, `FetchRequest`, `SearchRequest`, `AppendRequest`, `PluginItem`, `SubscriptionRequest` |
| `protocol/{agent,agent_execution,agent_lifecycle,controls,creation,native_sessions,resume}` | —                                               | whole                                                                                                                      |

Coupling verified at `f3a2c3d3` by scanning every `crate::` and `super::` path in `crates/provider/src`:

- **Adapter to runtime, production:** only `service::bot_execution::{BotSessionSupport, InstructionDelivery}`, imported by `ports/agent_session.rs:12`, `local/claude.rs:28` and `local/codex.rs:40`.
- **Runtime to adapter, production:** `local::pi::PROVIDER` at `rpc/agent_execution/native_sessions.rs:135` and `service/agent_manager/native_sessions.rs:393`, and the seven client types in `composition.rs`.
- **Adapter to adapter, production:** none. **Runtime to shared toolkit, production:** none, so the runtime will not depend on `provider-native`.
- **Runtime tests that build a non-Codex adapter:** `service/provider_catalog/tests.rs:173` and `:196`, `service/provider_catalog/tests/diagnostics.rs:57`, `service/agent_execution/tests/subscriptions/creation.rs`, `testing/tests.rs`, `composition/tests.rs`. **Toolkit test that builds an adapter:** `local/summary_model/tests.rs:32`.
- **Adapter tests that reach runtime internals:** about 60 files. They use `AgentManager` (`register_client`, `close_all`, `send`, `poll`, `active_turn`, `usage`, `last_message`, `send_input`, `load_timeline`, `has_sessions`, `fork`, `close`, `cancel`, `subagents`, `resume`, `restore_new`, `live_snapshot`, `events`, `configure`, `commands`), `AgentManagerError`, `AgentRegistration`, `ownership::Owners`, `Timeline::{memory, open}`, `MAX_ENTRY_BYTES`, `AgentExecution`, `AgentMcp`, `AgentRuntimeDirectory`, `effective_thinking_option_id`, `provider_catalog::Catalog`, `rpc::timeline::fetch_stored`, `timeline_projection::project`, the runtime-only request types `protocol::resume::Overrides` and `protocol::timeline::FetchRequest`, `test_support::{Fixture, worker_with_client, request}` and the `cfg(test)` module `local/agent_mcp_injection.rs`.
- **Orphan rule:** no `impl` block for a future `provider-core` type lives outside the files that move with it.

Decisions the executor must not reopen:

1. **The runtime keeps the name `provider`.** Renaming it would touch 43 files in `bot`, `api` and `daemon` and the `paths` of 84 ADRs for no gain.
2. **No production edge between an adapter and the runtime, in either direction.** An adapter's production dependencies are `provider-core`, `provider-native` and `domain` (`model` only for `provider-codex`, which uses `model::realtime`). This also makes decision 5 possible: an adapter that linked the runtime could not be used from the runtime's own tests, because the test build of `provider` and the copy the adapter links would define two different `AgentClient` traits.
3. **`Providers` stays in the runtime as a container; `provider-builtin` owns the list.** `Providers::with` (added by ADR-249) already accepts adapters from other crates. While adapters still live in `provider`, `Providers::configured` builds exactly those and `provider_builtin::configured` appends the extracted ones; when the last adapter leaves, `configured` is replaced by `Providers::empty`. `summary_generation::Generation` and `UsageLedger` stay crate-private, and no adapter module becomes `pub` temporarily. Registration order is not observable: the summary generator and the catalog key adapters by name.
4. **Adapter tests stay with the adapter and reach the runtime only through `provider::testing`** (feature `test-support`, ADR-249 §4). A missing operation gets a delegating wrapper there; a runtime item is never widened to `pub` for a test. `provider::testing` may use `persistence` through an optional dependency that only `test-support` enables, so the existing persistence-backed helpers move there unchanged.
5. **`provider` dev-depends on exactly one adapter, `provider-codex`.** The 131-test `service/agent_execution/tests` suite and the manager tests drive the Codex Python fake. Any other adapter in the runtime's dev-dependencies would make every change to it select the whole family, because `verify.mjs` follows dev edges.
6. **A fixture lives with the crate that owns the fake or produces the data.** Fake native programs and adapter-specific JSON move to `crates/provider-<adapter>/tests/fixtures/`; `legacy_tool_details.json` and the three `paseo-*.json` files stay in `crates/provider/tests/fixtures/`. This replaces the ADR-249 §4 exception for the two OMP JSON files.
7. **A runtime test that builds a non-Codex adapter is split.** The runtime half uses a stub `AgentClient` (as `Client` and `ImportClient` already do in those test modules); the adapter half asserts the adapter's own port values in the adapter's tests. A case that needs several real adapters and the real runtime together moves to `provider-builtin`.
8. **Gate:** after Task 6 (first adapter extracted with the recipe) compare its measurements with the table above before starting Task 7.

Not in scope: splitting the runtime further (`skills`, `storage`, `timeline_projection` stay); removing the seven string-keyed provider special cases in runtime code (`service/provider_catalog.rs:248-273`, `service/agent_manager/native_sessions.rs:348`, `service/agent_manager/recovery.rs:84`, `service/agent_manager/controls.rs:256`, `diagnostics.rs:51-66`), which are not compile-time edges; replacing the Codex fake in the runtime suite with a scripted client.

## Global Constraints

- Read [`docs/policy/rust.md`](../policy/rust.md) before any Rust change; `unsafe_code = "forbid"`, `missing_docs`, `unreachable_pub` and `unused_crate_dependencies` are all denied by `cargo clippy ... -D warnings`.
- A library crate exposes only items another workspace crate uses ([ADR-116](../decisions/daemon/adr-116-crate-visibility.md)). Widen visibility one compiler error at a time and add the doc comment `missing_docs` then asks for; never make a module tree `pub` wholesale. No forwarding `pub use` ([ADR-107](../decisions/daemon/adr-107-direct-imports-from-owning-crates.md)).
- No behavior, wire format, persisted format, environment variable or provider id changes. If a test needs editing beyond import paths, visibility and the facade calls of decision 4, stop and report it.
- Move whole files with `git mv` and limit edits in a moved file to paths, visibility and doc comments. PRs are squash-merged, so the check is on the PR diff: `git diff -M --summary origin/main... | grep -c '^ rename'` equals the number of files moved.
- Path rewrites are scripted (`perl -pi` over `git ls-files '*.rs'`) and the exact commands go in the PR description, so branches in flight can replay them after rebasing. The file counts in the tasks come from single-line matches. Grouped imports need a manual pass: 29 files use `use crate::{...}`, 12 more use `crate::ports::{...}` or `crate::protocol::{...}`, and `service/agent_manager/delivery.rs:4` mixes a staying and a moving module in one group. `cargo check --workspace --all-targets` finds what the script missed.
- Test count parity: `cargo nextest list -p <every provider family crate> --message-format oneline | wc -l` never drops below the previous task's figure. The baseline is 1,696 + 245 = 1,941 on macOS; a drop means a `mod tests;` line was lost in a move.
- New crates live at `crates/<name>`; `members = ["bins/*", "crates/*"]` picks them up. Add each to `[workspace.dependencies]` in the root `Cargo.toml`, and copy `[package]` and `[lints]` from `crates/provider-omp/Cargo.toml`.
- Each new crate's path is appended to `paths` in the family ADR and in every ADR about the code that moved, because `node scripts/adr.mjs governs` matches whole path segments and `crates/provider` does not cover `crates/provider-codex`. Regenerate the index with `node scripts/adr.mjs index` and run `npm run check:docs`.
- Verification: Tasks 1 and 2 run `npm run verify -- --scope closure`. From Task 3 on every PR edits the root `Cargo.toml` and `Cargo.lock`, which `verify.mjs` treats as workspace-wide, so that command would run the full suite AGENTS.md reserves for an explicit user request; CI runs it on the PR regardless. Locally run the **family check** instead: `npm run verify -- --only docs,format`, then `cargo clippy --locked --all-targets -- -D warnings` and `cargo nextest run --locked`, each with `-p` for every provider family crate plus `bot`, `api` and `daemon`.
- macOS-local green is not sufficient for Tasks 4, 7 and 8: `sandbox/linux` and several adapters have `cfg(target_os = "linux")` code. Wait for the CI Rust job before merging.
- One PR per task, and one per adapter in Task 7. Land each within a day of opening it and announce the adapter beforehand; this area takes about four commits a day.

## Review Focus

1. A `mod tests;` declaration lost in a move silently removes tests. Pinned by the test-count step in Tasks 3–8.
2. `test-support`, or the `persistence` dependency behind it, enabled in a release build would ship test entry points. Pinned by the guard rules in Task 1 and the `cargo tree` step in Tasks 6–8.
3. A fixture path that still points at `crates/provider/tests/fixtures/` fails only at test run time, and for TypeScript, the desktop e2e and the mobile e2e only in the UI job. Pinned by the consumer tables and the stale-path step in Tasks 4, 6, 7 and 8.
4. An item gated `#[cfg(test)]` in one crate and called from another fails to compile only in the test profile. Pinned by running `cargo clippy --all-targets` and a featureless `cargo clippy -p provider --lib` in Tasks 4–8.
5. A new production edge between an adapter and the runtime added by an unrelated PR while the split is in flight. Pinned by the source-scan test added in Task 2 and removed in Task 9.

---

### Task 1: ADR, guard rules and baseline

**Files:**

- Create: `docs/decisions/providers/adr-<N>-provider-crate-family.md`. `<N>` is a number no ref uses; on 2026-11-12 `main` ends at ADR-249 and open branches hold ADR-256 and ADR-265. Check every ref before picking and again before merging.
- Modify: `docs/decisions/README.md` (generated), `bins/daemon/tests/dependencies.rs`

**Interfaces:**

- Produces: guard allow-lists for every crate name in the Design table, a dev-only edge rule and the optional-`persistence` rule that later tasks rely on.

- [ ] **Step 1: Record the baseline** in the PR description: the Why table re-measured at the PR's base commit, and the two `cargo nextest list` counts.

- [ ] **Step 2: Write the failing guard test** in `bins/daemon/tests/dependencies.rs`:

```rust
#[test]
fn provider_family_keeps_adapters_and_runtime_apart() {
    let package = |name: &str, dependencies: Value| {
        json!({"id": name, "name": name, "dependencies": dependencies})
    };
    let edge = |name: &str, kind: Value| json!({"name": name, "path": "x", "kind": kind});
    let packages = [
        package("provider-core", json!([edge("provider", Value::Null)])),
        package("provider-native", json!([edge("provider-core", Value::Null)])),
        package(
            "provider",
            json!([
                edge("provider-core", Value::Null),
                edge("provider-codex", json!("dev")),
                edge("provider-pi", json!("dev")),
            ]),
        ),
        package("provider-codex", json!([edge("provider", json!("dev"))])),
        package("provider-pi", json!([edge("provider", Value::Null)])),
        package(
            "provider-builtin",
            json!([edge("provider", Value::Null), edge("provider-pi", Value::Null)]),
        ),
    ];
    assert_eq!(
        violations(&packages),
        [
            "provider-core -> provider",
            "provider -> provider-pi",
            "provider-pi -> provider outside tests",
        ]
    );
    let runtime = [
        package("provider", json!([edge("provider-codex", Value::Null)])),
        package("provider-codex", json!([])),
    ];
    assert_eq!(violations(&runtime), ["provider -> provider-codex outside tests"]);
}

#[test]
fn runtime_reaches_persistence_only_through_test_support() {
    let optional = json!({"name": "persistence", "path": "x", "kind": null, "optional": true});
    for (feature, expected) in [
        ("test-support", vec![]),
        ("default", vec!["provider -> persistence outside tests"]),
    ] {
        let packages = [
            json!({"id": "provider", "name": "provider", "dependencies": [optional.clone()],
                "features": {feature: ["dep:persistence"]}}),
            json!({"id": "persistence", "name": "persistence", "dependencies": []}),
        ];
        assert_eq!(violations(&packages), expected);
    }
}
```

- [ ] **Step 3: Run it.** `cargo nextest run -p daemon --test dependencies` — expected: both new tests FAIL, the first with `unregistered workspace package: provider-core`.

- [ ] **Step 4: Implement the rules** in `violations`. Replace the hard-coded `persistence outside tests` branch with one table of dev-only edges so there is a single mechanism.

| Package                       | Allowed workspace dependencies                                                             | Must be `kind == "dev"`                                                                                                        |
| ----------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `provider-core`               | `domain`, `model`                                                                          | —                                                                                                                              |
| `provider-native`             | `domain`, `provider-core`                                                                  | —                                                                                                                              |
| each new `provider-<adapter>` | `domain`, `model`, `provider-core`, `provider-native`, `provider`, `persistence`           | `provider`, `persistence`, and `model` except `provider-codex`                                                                 |
| `provider-omp`                | unchanged in this task; Task 3 adds `provider-core`, Task 4 switches it to the adapter row | as today                                                                                                                       |
| `provider`                    | `domain`, `model`, `persistence`, `provider-core`, `provider-native`, `provider-codex`     | `provider-codex`; `persistence`, unless the edge is optional, `test-support` lists `dep:persistence` and no other feature does |
| `provider-builtin`            | `domain`, `model`, `persistence`, `provider`, `provider-core`, the eight adapters          | `persistence`                                                                                                                  |
| `bot`                         | existing list plus `provider-core`                                                         | —                                                                                                                              |
| `api`                         | existing list plus `provider-core`, `provider-builtin`                                     | `provider-builtin`                                                                                                             |
| `daemon`                      | existing list plus `provider-core`, `provider-builtin`; Task 5 removes `provider-omp`      | —                                                                                                                              |

- [ ] **Step 5: Run it.** Same command — expected: PASS, including the twenty existing guard tests. `only_the_daemon_injects_persistence_adapters_outside_tests` already feeds an optional `persistence` edge without a feature map and must keep reporting it.

- [ ] **Step 6: Write the ADR** with the Design section's graph, the two tables and the eight decisions; status `accepted`; `amends` lists ADR-089 (assembly moves to `provider-builtin`, the OMP exception ends), ADR-107 (§4: `persistence` behind `test-support`), ADR-179 (an adapter's tests live in its crate) and ADR-249 (§1 dependencies, §2 daemon registration, §4 fixtures); `paths` lists `crates/provider`, `crates/provider-omp` and `bins/daemon`. Run `node scripts/adr.mjs index` and `npm run check:docs`.

- [ ] **Step 7: Commit** as `docs(provider): 记录 provider crate 拆分决策并加入依赖守卫`.

### Task 2: Remove the cross-layer edges in place

**Files:**

- Create: `crates/provider/src/ports/bot_session.rs`, `crates/provider/src/tests.rs`
- Modify: `crates/provider/src/{lib.rs,ports.rs}`, `ports/agent_session.rs`, `ports/native_history.rs`, `service/bot_execution.rs`, `service/agent_manager.rs`, `service/agent_manager/native_sessions.rs` and its `tests.rs`, `rpc/agent_execution/native_sessions.rs` and its `tests.rs`, `rpc/agent_execution/bot.rs` and `bot/tests.rs`, `rpc/agent_execution/resume.rs`, `local/claude.rs`, `local/codex.rs`, `local/pi.rs`, `local/summary_model/tests.rs`, `local/deepseek_harness/tests.rs`, `testing/tests.rs`, `crates/bot/src/execution.rs`

**Interfaces:**

- Produces: `ports::bot_session::{BotSessionSupport, InstructionDelivery}`, moved verbatim with the same derives and serde attributes; `ports::native_history::CwdLookup`; `AgentClient::cwd_lookup(&self) -> CwdLookup`; `AgentManager::cwd_lookup(&self, provider: &str) -> CwdLookup`.

- [ ] **Step 1: Write the failing boundary test** in `crates/provider/src/tests.rs`, declared from `lib.rs` as `#[cfg(test)] mod tests;`. One test, `layers_reference_only_what_their_future_crate_may_depend_on`, asserts that a source scan returns no `"<relative path>:<line>"` entries for these rules:
  1. A non-test file under `local/` references no runtime module (`service`, `rpc`, `storage`, `dispatch`, `connection`, `support`, `timeline_projection`, `skills`, `capabilities`, `composition`, `installation`, `diagnostics`, `testing`).
  2. A file in the `provider-core` column of the contract table references nothing outside that column.
  3. A file of the eighteen toolkit modules, tests included, references no adapter module.
  4. A non-test file outside `local/` references `crate::local` only from `composition.rs`.
  5. A test file outside `local/` references no adapter except `local::codex`, apart from a `const PENDING: &[&str]` list holding `service/provider_catalog/tests.rs`, `service/provider_catalog/tests/diagnostics.rs`, `service/agent_execution/tests/subscriptions/creation.rs` and `composition/tests.rs`.

  A test file is any path with a `tests` or `fixtures` component, `tests.rs`, `test_support*` and `local/agent_mcp_injection.rs`. The scan skips lines whose trimmed text starts with `//`, matches `crate::<name>::`, and inside a `use crate::{ ... };` block matches an entry that is `<name>` or starts with `<name>::`.

- [ ] **Step 2: Run it.** `cargo nextest run -p provider -E 'test(layers_reference_only)'` — expected: FAIL listing `ports/agent_session.rs:12`, `local/claude.rs:28`, `local/codex.rs:40`, `rpc/agent_execution/native_sessions.rs:135`, `service/agent_manager/native_sessions.rs:393`, the two `native_sessions/tests.rs` files, `testing/tests.rs:5` and `local/summary_model/tests.rs:32`.

- [ ] **Step 3: Move the bot session types.** Cut both from `service/bot_execution.rs` into `ports/bot_session.rs`, add `pub mod bot_session;` to `ports.rs` and update every import, including `rpc/agent_execution/bot/tests.rs:6` and `crates/bot/src/execution.rs:7`. Leave no re-export behind.

- [ ] **Step 4: Replace the Pi comparisons with a capability.** Add to `ports/native_history.rs`:

```rust
/// Working-directory spelling a provider's native session lookup expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CwdLookup {
    /// The canonical directory Ait stores for the Agent.
    #[default]
    Canonical,
    /// The spelling the client sent, before canonicalization.
    Requested,
}
```

Add `fn cwd_lookup(&self) -> CwdLookup { CwdLookup::Canonical }` to `AgentClient`; `PiClient` overrides it with `Requested`. Add `pub(crate) fn cwd_lookup(&self, provider: &str) -> CwdLookup` to `AgentManager`, returning `Canonical` for an unregistered provider. Change the signatures to `inspection_cwd<'a>(lookup: CwdLookup, requested_cwd: &'a str, canonical_cwd: &'a str) -> &'a str` and `listing_options(lookup: CwdLookup, options: &ListOptions, lookup_cwd: Option<&str>) -> ListOptions`; the three call sites (`rpc/agent_execution/native_sessions.rs:68`, `rpc/agent_execution/resume.rs:84`, `service/agent_manager/native_sessions.rs:59`) obtain `lookup` from the manager. In `service/agent_manager/native_sessions/tests.rs` the stub `ImportClient` returns `Requested` and names its provider with the literal `"pi"`.

- [ ] **Step 5: Pin the capability.** In `rpc/agent_execution/native_sessions/tests.rs`:

```rust
#[test]
fn inspection_uses_the_spelling_the_provider_asks_for() {
    assert_eq!(inspection_cwd(CwdLookup::Requested, "/a/link", "/a/real"), "/a/link");
    assert_eq!(inspection_cwd(CwdLookup::Canonical, "/a/link", "/a/real"), "/a/real");
}
```

and in the Pi adapter's tests, `assert_eq!(PiClient::new("pi".into()).cwd_lookup(), CwdLookup::Requested);`.

- [ ] **Step 6: Clear the two test edges.** Move `model_family_names_do_not_match_unrelated_small_model_tokens` from `local/summary_model/tests.rs` to the DSH adapter's tests. In `testing/tests.rs` register `CodexClient::new("codex".into())` instead of `PiClient` and expect `AlreadyExists("codex")`.

- [ ] **Step 7: Run.** `cargo nextest run -p provider -E 'test(layers_reference_only) | test(inspection_uses) | test(native_sessions) | test(cwd_lookup)'` — expected: PASS. Then `npm run verify -- --scope closure`.

- [ ] **Step 8: Commit** as `refactor(provider): 移除适配器与运行时之间的生产依赖`.

### Task 3: Extract `provider-core`

**Files:**

- Create: `crates/provider-core/Cargo.toml`, `crates/provider-core/src/{lib.rs,ports.rs,protocol.rs}`, `crates/provider-core/src/ports/usage_ledger.rs`, `crates/provider-core/src/protocol/{provider.rs,timeline.rs}`
- Move: the whole modules of the contract table into `crates/provider-core/src/{ports,protocol}/`
- Modify: root `Cargo.toml`, `crates/provider/Cargo.toml`, `crates/provider/src/{ports.rs,protocol.rs}`, the three split modules, `crates/provider-omp/Cargo.toml` and its imports, `crates/bot/Cargo.toml`, `crates/bot/src/execution.rs`, the guard row for `provider-omp`

**Interfaces:**

- Produces: `provider_core::ports::{agent_session, account_usage, bot_session, controls, diagnostics, environment, native_history, usage_ledger}` and `provider_core::protocol::{agent_config, prompt, provider, timeline, usage}`, all `pub` modules. `provider::ports` keeps `agent`, `mcp_scope`, `usage_ledger`; `provider::protocol` keeps the modules in the right-hand column.

- [ ] **Step 1: Create the crate skeleton.** `lib.rs`:

```rust
//! Contracts shared by the provider runtime and the native adapters.
//!
//! Adapters implement these ports; the runtime drives them. Neither side depends on the other.

pub mod ports;
pub mod protocol;
```

- [ ] **Step 2: Move the whole modules with `git mv`** and cut the split items (`UsageBoundary`, `RequestUsage`, `Details`, `NativeItem`, with the tests that exercise only them) into the three new files. `ports/usage_ledger/tests.rs` has one test and a `request()` helper that cover both halves; split that file by hand.

- [ ] **Step 3: Rewrite paths** in `crates/provider/src` (194 files reference a moved `ports` module, 103 a moved `protocol` module), `crates/provider-omp/src` and `crates/bot/src`: `crate::ports::<moved>` and `provider::ports::<moved>` become `provider_core::ports::<moved>`, likewise for `protocol`. For the three split modules rewrite per item, for example `crate::protocol::timeline::NativeItem` moves and `crate::protocol::timeline::FetchRequest` stays. The `protocol` modules that stay reach moved ones through `super::`: `agent_execution.rs:33,101,112` (`super::prompt`), `agent_lifecycle.rs:9` and `resume.rs:6` (`super::agent_config`).

- [ ] **Step 4: Make it compile.** Change `pub(crate)` to `pub` only where `cargo check --workspace --all-targets` reports a privacy error. Start `provider-core`'s dependency list from `provider`'s and delete entries until `unused_crate_dependencies` is silent in both crates. A test in a moved module that uses `crate::rpc` or `crate::service` stays in `provider`, next to the module it exercises.

- [ ] **Step 5: Shrink the boundary test.** Drop rule 2; Cargo now enforces it.

- [ ] **Step 6: Verify.** The family check passes; the family test count is at least the baseline plus the tests added in Task 2; the rename check of Global Constraints holds; `provider-core` is appended to the `paths` of the family ADR and of ADR-205. In `scripts/paseo-focused-coverage.py:19-25` the `ports::environment::` and `ports::agent_session::` filters move from the `provider` scope to a `provider-core` scope.

- [ ] **Step 7: Commit** as `refactor(provider): 提取 provider-core 契约 crate`.

### Task 4: Extract `provider-native` and re-point `provider-omp`

**Files:**

- Create: `crates/provider-native/Cargo.toml`, `crates/provider-native/src/lib.rs`
- Move: the eighteen toolkit modules to `crates/provider-native/src/`; `crates/provider/tests/fixtures/linux_sandbox_{probe,terminal}.py` to `crates/provider-native/tests/fixtures/`; `crates/provider/tests/fixtures/omp_{dialogs,question}.json` to `crates/provider-omp/tests/fixtures/`
- Modify: `crates/provider/src/local.rs`, `crates/provider/Cargo.toml`, `crates/provider-omp/Cargo.toml` and its imports, `scripts/verify.mjs`, `scripts/verify.test.mjs`, `packages/protocol/src/messages.omp-{dialogs,question}.test.ts`, `apps/mobile/src/components/question-form-card.test.tsx`, the guard row for `provider-omp` and the expectation of `runtime_test_entry_points_stay_out_of_production_builds`

**Interfaces:**

- Consumes: `provider_core::{ports, protocol}`.
- Produces: `provider_native::<module>` for each moved module; only items an adapter uses are `pub`. `provider-omp`'s production dependencies become `provider-core`, `provider-native` and `domain`; `provider` remains only as its `test-support` dev-dependency.

- [ ] **Step 1: Generalize the shared-fixture rule first.** In `scripts/verify.test.mjs` change the existing `omp_question.json` case to `crates/provider-omp/tests/fixtures/omp_question.json`, keeping its expectation. Run `node --test scripts/verify.test.mjs` — expected: FAIL. In `scripts/verify.mjs:197` replace the `startsWith("crates/provider/tests/fixtures/")` test with `/^crates\/provider(?:-[a-z-]+)?\/tests\/fixtures\//`. Run again — expected: PASS.

- [ ] **Step 2: Skeleton and `git mv`.** `lib.rs` carries the crate doc `//! Process, sandbox, ACP and media helpers shared by native provider adapters.` and one `pub mod` per moved module. `nix` moves here as the `cfg(unix)` dependency.

- [ ] **Step 3: Rewrite paths.** `crate::local::<module>` (83 files), `super::<module>` inside `local/` (13 files) and `provider::local::<module>` in `provider-omp` become `provider_native::<module>`. Apply the `super::<module>` rewrite only to `local/*.rs`: `local/antigravity/diagnostics.rs`, `local/codex/sandbox.rs` and `local/deepseek_harness/native/usage.rs` are adapter modules that share a toolkit module's name. Widen and document as in Task 3 Step 4; the toolkit holds about 130 `pub(super)` and `pub(crate)` items, and only those an adapter uses become `pub`. Drop rule 3 from the boundary test; Cargo now enforces it.

- [ ] **Step 4: Re-point `provider-omp`.** Remove `provider` from `[dependencies]`, switch the guard to the adapter row, delete the hard-coded `provider-omp` to `model` branch in `test_only_violations` (`bins/daemon/tests/dependencies.rs:72`) that the row now covers, and update the existing guard test, which now also expects `provider-omp -> provider outside tests` for the non-dev cases.

- [ ] **Step 5: Stale-path check.** `git grep -nE 'provider/tests/fixtures/(linux_sandbox|omp_)' -- ':!docs'` prints nothing. The pattern has no `crates/` prefix because `provider-omp` reaches the two JSON files through `../../../provider/tests/fixtures/` at `interactions/tests.rs:53,91`, `tests/interactions.rs:6` and `tests/installed/interactions.rs:186`.

- [ ] **Step 6: Verify.** The family check; `cargo tree -p provider-omp -e normal --depth 1` lists no `provider`; `cargo clippy --locked -p provider --lib -- -D warnings` passes without features; the test count holds. Append `crates/provider-native` to the `paths` of the family ADR, `workspace/adr-215-sandboxed-worktree-isolation.md` and `workspace/adr-223-linux-worktree-sandbox.md` (both numbers are shared with another ADR). Wait for the CI Rust job (Linux sandbox tests).

- [ ] **Step 7: Commit** as `refactor(provider): 提取 provider-native 并让 provider-omp 脱离运行时`.

### Task 5: Add `provider-builtin`

**Files:**

- Create: `crates/provider-builtin/Cargo.toml`, `crates/provider-builtin/src/{lib.rs,tests.rs}`
- Modify: `crates/provider/src/composition.rs`, `crates/provider/src/composition/tests.rs`, `crates/provider/src/testing.rs`, `service/provider_catalog/tests.rs`, `service/provider_catalog/tests/diagnostics.rs`, `bins/daemon/src/host.rs:743-757` (`compose_providers`) and its `provider_omp` import at `:57`, `bins/daemon/Cargo.toml`, `bins/daemon/tests/{process.rs,dependencies.rs}` (their `use provider_omp as _;` lines), `crates/api/Cargo.toml`, `crates/api/tests/fragmentation.rs` (it lists every `api` dependency as `use x as _;` and needs `use provider_builtin as _;`), `crates/api/src/tests/mcp/scoped/support.rs:296`, the guard row for `daemon`

**Interfaces:**

- Consumes: `provider::Providers::{configured, with, with_usage_ledger}`, `provider_omp::{OmpClient, installed_program}`; in tests, `provider` with `test-support`.
- Produces:

```rust
/// Built-in adapters configured from the process environment and installed native programs.
pub fn providers(data_dir: &Path) -> Providers;

/// Like [`providers`], reading launcher overrides such as `AIT_SERVER_CODEX_BIN` from `environment`.
#[doc(hidden)]
pub fn configured(data_dir: &Path, environment: impl Fn(&str) -> Option<OsString>) -> Providers;
```

`Providers::new` is deleted so that every caller fails to compile until it is switched (`bins/daemon/src/host.rs:747` and two tests). The moved tests read the assembly through new `provider::testing` entry points, because `Providers`' fields and `AgentManager::validate_provider` are crate-private:

```rust
/// Adapters in `providers`, in assembly order.
pub fn clients(providers: &Providers) -> &[Box<dyn AgentClient>];
/// The adapters of `providers` that implement structured auxiliary generation.
pub fn summary_clients(providers: &Providers) -> &[Arc<dyn AgentClient>];
impl Manager {
    /// Check that `provider` names a registered adapter.
    pub fn validate_provider(&self, provider: &str) -> Result<(), AgentManagerError>;
}
```

- [ ] **Step 1: Write the failing test** in `crates/provider-builtin/src/tests.rs`: `the_built_in_set_registers_every_adapter_once`, moved from `registration_preserves_the_catalog_without_startup_writes_and_rejects_duplicates`, asserting that `configured(dir, |_| None)` registers exactly `antigravity`, `claude`, `codex`, `deepseek-harness`, `minimax-code`, `omp`, `opencode`, `pi`, writes nothing under `dir`, and that registering the result twice returns `AlreadyExists`.

- [ ] **Step 2: Run it.** `cargo nextest run -p provider-builtin` — expected: FAIL to compile, `configured` not found.

- [ ] **Step 3: Implement.** `configured` calls `Providers::configured(data_dir, &environment)` and appends OMP exactly as `compose_providers` does today. The daemon calls `provider_builtin::providers(data_dir)` and drops its `provider-omp` dependency.

- [ ] **Step 4: Re-home the assembly tests.** Move the whole-set cases of `composition/tests.rs` here: `explicit_missing_executables_do_not_fall_back_to_installed_programs`, `dsh_transport_override_preserves_native_and_acp_capabilities` and `configured_codex_and_claude_generate_summary_without_foreground_agents`. The last one replaces `test_support::Fixture` with its own temporary directory, a symlink to `codex_app_server.py` and a local `SummaryConfiguration` implementation; it names both fake programs by path, and Tasks 7 and 8 update those paths. Two cases stay in `provider`: `summary_generators_record_their_calls_in_the_installed_host_ledger`, and `external_adapters_join_the_assembly_with_the_shared_image_directory`, rewritten to append a stub `Clone` client instead of `PiClient` and to assert one more client than before instead of the literal `8`.

- [ ] **Step 5: Apply decision 7 to the pending runtime tests.** Split `native_import_support_does_not_require_an_installed_executable`, `snapshot_entries_publish_bot_session_support` and `available_list_exposes_unsupported_pi_version_guidance`: the catalog half keeps the stub `Client`, and each adapter asserts its own `bot_session_support()`, import support and availability in its tests. Remove the three files from `PENDING`; `creation.rs` stays until the DSH adapter moves.

- [ ] **Step 6: Verify.** The family check; `cargo tree -p daemon -e normal --depth 1` lists `provider-builtin` and not `provider-omp`. Append `crates/provider-builtin` to the `paths` of the family ADR and ADR-089.

- [ ] **Step 7: Commit** as `refactor(provider): 新增 provider-builtin 组装 crate`.

### Task 6: Extract `provider-minimax` (pilot) and measure

**Files:**

- Create: `crates/provider-minimax/Cargo.toml`
- Move: `crates/provider/src/local/minimax.rs` to `crates/provider-minimax/src/lib.rs`; `local/minimax/*` to `crates/provider-minimax/src/`
- Modify: `crates/provider/src/{local.rs,composition.rs,testing.rs,test_support.rs}`, `crates/provider/Cargo.toml`, `crates/provider-builtin/{Cargo.toml,src/lib.rs}`, `apps/mobile/e2e/browser/acp-provider-catalog.spec.ts` (the fake becomes `crates/provider-minimax/src/tests/fixtures/acp.py`)

**Interfaces:**

- Consumes: `provider_core`, `provider_native`; in tests, `provider` with `test-support`.
- Produces: `provider_minimax::MiniMaxClient` with `new(program: PathBuf)` and `with_image_directory(PathBuf)`. In `provider::testing`, the generic half of `test_support.rs`, with `persistence` declared as `optional = true` and `test-support = ["dep:persistence"]`:

```rust
/// Temporary data root and working directory of one execution test.
pub struct Workbench { pub root: tempfile::TempDir, pub cwd: PathBuf }
impl Workbench { pub fn new() -> Self }

/// Start an execution worker over file-backed registries in `bench`, with `client` registered.
pub fn worker(
    bench: &Workbench,
    creations: model::creation::Creations,
    client: Box<dyn AgentClient>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry);

/// Execute `method` and return its result, failing the test on an error or after 30 seconds.
pub async fn request(execution: &AgentExecution, method: &str, params: Value) -> Value;
```

`worker` and `test_support::worker_with_ledger` both call one crate-private helper, `testing::spawn(root: &Path, cwd: &Path, creations, client, configure: impl FnOnce(AgentManager) -> AgentManager)`, so the runtime's own suite keeps one implementation. The ledger variant passes `|manager| manager.with_ledger(ledger)`; `with_ledger` and `UsageLedger` stay test-only and crate-private.

The recipe, reused by Tasks 7 and 8:

- [ ] **Step 1: `git mv`** the adapter into its crate and write the manifest: `provider-core`, `provider-native`, `domain` and the third-party crates the compiler asks for; dev-dependencies `provider` with `test-support`, `persistence`, `model`.
- [ ] **Step 2: Rewrite paths in the moved tree.** `crate::local::<adapter>::` becomes `crate::`; `pub(in crate::local::<adapter>)` and any `pub(super)` in the new `lib.rs` become `pub(crate)`; the client type, its constructors and builder methods become `pub` with doc comments.
- [ ] **Step 3: Compile production code.** `cargo check -p provider-<adapter>`; delete dependencies until `unused_crate_dependencies` is silent.
- [ ] **Step 4: Compile tests.** `cargo check -p provider-<adapter> --tests`. Each use of `crate::service`, `crate::storage`, `crate::rpc`, `crate::timeline_projection` or `crate::test_support` becomes a `provider::testing` call, adding a delegating wrapper when one is missing (decision 4). A test that asserts runtime behavior rather than the adapter's follows decision 7.
- [ ] **Step 5: Re-home the registration.** Delete the module from `provider::local` and the client from `Providers::configured`; append it in `provider_builtin::configured` with the same environment variable, default program and builder calls.
- [ ] **Step 6: Move the fixtures** this adapter owns and update every consumer in the Task 7 table. An `include_str!` into `tests/fixtures/` is relative to its source file and changes depth with the move (`local/pi/tests/tool_schema.rs:20`, `local/antigravity/streaming/tests.rs:46`, `local/opencode/tool/tests.rs:5`). `scripts/fixtures/paseo/paseo-api-contracts.json` and `scripts/paseo-focused-coverage.py:19-25` name source paths and module filters of the Claude, OpenCode, DSH and Codex adapters; update the entries of the adapter that moves.
- [ ] **Step 7: Stale-path check.** `git grep -nE 'local/<adapter>|<each moved fixture file name>' -- ':!docs' ':!crates/provider-<adapter>'` prints only lines that resolve to the new location. Search by file name, not by full path: tests build these paths from `CARGO_MANIFEST_DIR` or with `../` segments.
- [ ] **Step 8: Verify.** The family check; featureless `cargo clippy --locked -p provider --lib -- -D warnings`; `cargo tree -p daemon -e features,no-dev | grep test-support` prints nothing; the family test count holds; `crates/provider-<adapter>` is appended to the `paths` of the family ADR and of every ADR found by `grep -il '<adapter name>' docs/decisions/*/adr-*.md` that lists `crates/provider`.
- [ ] **Step 9: Commit** as `refactor(provider): 提取 provider-<adapter> 适配器 crate`.

- [ ] **Step 10 (pilot only): Measure and report in the PR description.** With the extraction committed, make a one-line uncommitted edit in `crates/provider-minimax/src/lib.rs`; `node scripts/verify.mjs --plan --scope closure --base HEAD` must name `provider-minimax`, `provider-builtin`, `api` and `daemon` and must not name `provider` or any other adapter. Without `--base HEAD` the plan diffs against the merge-base, sees the manifest change and prints `--workspace`. Record the wall time of `cargo nextest run -p provider-minimax --no-run` and of `cargo nextest run -p provider --no-run` after the same edit pattern as the Why table.

**Gate:** proceed to Task 7 only if Step 10's selection is as stated and the adapter's test build is under 10 s. If `provider` appears in the selection, a dev edge from the runtime to this adapter survived; fix that first.

### Task 7: Extract the remaining adapters except Codex

One PR per adapter, each following Task 6 Steps 1–9 with the names substituted. The order is by size; the PRs conflict only in `local.rs`, `composition.rs`, `provider-builtin` and the root manifest, so they may be prepared in parallel and landed one at a time.

| Order | Crate                       | Single-owner dependencies that leave `provider` | Fixtures that move to `<crate>/tests/fixtures/`                                                                                                          | External consumers to update                                                                                                                                                                                                                   | Runtime items its tests reach beyond `Manager`                                                                                          |
| ----- | --------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | `provider-antigravity`      | —                                               | `antigravity_tool_cards.json`                                                                                                                            | `packages/protocol/src/messages.tool-call-schema.test.ts`                                                                                                                                                                                      | `rpc::timeline::fetch_stored`, `Timeline`, `MAX_ENTRY_BYTES`                                                                            |
| 2     | `provider-claude`           | `unicode-normalization` (macOS)                 | `claude_code.py`, `claude_keychain.py`                                                                                                                   | `bins/daemon/tests/process/voice.rs`                                                                                                                                                                                                           | `worker`, `request`, `Timeline`                                                                                                         |
| 3     | `provider-deepseek-harness` | `tokio-tungstenite`, `serde_yaml_ng`            | `deepseek_harness_native.sh`, `dsh_metadata.cjs`                                                                                                         | —                                                                                                                                                                                                                                              | `worker`, `request`, `Timeline::open`, the MCP injection helper                                                                         |
| 4     | `provider-opencode`         | —                                               | `opencode_acp.py`, `opencode_acp_proxy.py`, `opencode_observation.py`, `opencode_subagents.py`, `opencode_tool_cards.json`, `opencode-tool-history.json` | `bins/daemon/tests/process/opencode.rs`, `bins/daemon/tests/process/agent_execution/workspace_creation/opencode.rs`, `packages/protocol/src/messages.tool-call-schema.test.ts`, `packages/protocol/src/messages.opencode-tool-history.test.ts` | `worker`, `request`, `ownership::Owners`, `provider_catalog::Catalog`, the MCP injection helper                                         |
| 5     | `provider-pi`               | `url`                                           | `pi_tool_cards.json`                                                                                                                                     | `packages/protocol/src/messages.tool-call-schema.test.ts`                                                                                                                                                                                      | `ExecutionDependencies`, `AgentRuntimeDirectory`, `effective_thinking_option_id`, `provider_catalog::Catalog`, the MCP injection helper |

Adapter-specific steps:

- **DSH.** The first of DSH, OpenCode and Pi to move also moves `local/agent_mcp_injection.rs` to `crates/provider/src/testing/agent_mcp.rs` (`pub mod agent_mcp;` in `testing.rs`), unchanged apart from visibility. `service/agent_execution/tests/subscriptions/creation.rs` leaves the runtime with this adapter: if its assertions are about subscription delivery, re-drive them with the Codex fake and keep the file; if they are about DSH's ACP behavior, move the cases to the adapter and expose the `Peer` harness through `provider::testing`. Then remove the file from `PENDING`.
- **OpenCode** is the only adapter besides the runtime that uses `rusqlite`; both keep it.
- **Pi** has the largest test tree (11.0k lines); budget its PR accordingly.

### Task 8: Extract `provider-codex`

**Files:**

- Create: `crates/provider-codex/Cargo.toml`, `crates/provider-codex/src/testing.rs`
- Move: `local/codex.rs`, `local/codex/*`; `crates/provider/src/test_support/codex_history.rs`; `crates/provider/tests/fixtures/codex_app_server.py` to `crates/provider-codex/tests/fixtures/`
- Modify: `crates/provider/src/{lib.rs,test_support.rs,composition.rs}`, `crates/provider/Cargo.toml`, the runtime tests that name `crate::local::codex` (`service/agent_execution/tests/{bot/profile.rs,concurrency.rs,usage.rs}`, `service/agent_manager/usage/host/tests.rs`, `service/provider_catalog/tests.rs`, `testing/tests.rs`), and the fixture consumers `bins/daemon/tests/process/native.rs`, `crates/api/src/tests/mcp/scoped/support.rs`, `packages/client/src/protocol/daemon.integration.test.ts`, `apps/desktop/e2e/rust-startup.e2e.mjs`, `apps/mobile/e2e/support/helpers/{e2e-worker.ts,schedule-host.ts}`
- Delete: `crates/provider/src/local.rs`

**Interfaces:**

- Produces: `provider_codex::CodexClient`; `provider_codex::testing::{Fixture, codex_history}` behind `#[cfg(all(unix, any(test, feature = "test-support")))]` (`Fixture::new` uses a Unix symlink), with `Fixture` keeping `new`, `spec`, `client`, `mode`, `requests` and the fields `root`, `program`, `cwd`; `CodexClient::with_fixture_media` takes the same gate; `provider-codex` declares `base64` as an optional dependency and a dev-dependency with `[features] test-support = ["dep:base64"]`, because `codex_history` needs it and Codex production code does not. `Providers::empty(data_dir: &Path) -> Self` replaces `Providers::configured`.

- [ ] **Step 1: Follow Task 6 Steps 1–9.** `webrtc`, `rtc`, `opus2` and `toml` leave `provider`; `model` is a normal dependency here.
- [ ] **Step 2: Split `test_support.rs`.** `Fixture` and `codex_history` go to `provider_codex::testing`; `worker_with_client`, `worker_with_ledger` and `request` stay and import `provider_codex::testing::Fixture`. `provider`'s dev-dependency is `provider-codex = { workspace = true, features = ["test-support"] }`. The two crates dev-depend on each other; Cargo accepts a cycle that consists only of dev edges.
- [ ] **Step 3: Empty the runtime.** Delete `local.rs`, replace `Providers::configured` with `Providers::empty` (the ledger case in `composition/tests.rs` builds `Providers::empty(..).with(..)` around `provider_codex::CodexClient`), and remove the dependencies `unused_crate_dependencies` now reports (`provider-native` among them).
- [ ] **Step 4: Verify** as Task 6 Step 8, and additionally `cargo tree -p provider -e normal --depth 1` lists no `provider-native` and no adapter; `PENDING` in the boundary test is empty.
- [ ] **Step 5: Commit** as `refactor(provider): 提取 provider-codex 适配器 crate`.

### Task 9: Close out

**Files:**

- Delete: `crates/provider/src/tests.rs` and its `mod tests;` line (Cargo now enforces every rule the scan held)
- Modify: `bins/daemon/tests/dependencies.rs` (remove `provider-native` from the `provider` row), `docs/architecture/README.md` (crate table, the dependency paragraph, the assembly paragraph and the test-ownership paragraph), `docs/architecture/provider.md`, `docs/architecture/omp.md`, `AGENTS.md` (the provider-test bullet), `docs/plans/README.md` (remove this plan's row), `third-party/paseo/NOTICE` (lines 8 and 24–34 name `crates/provider/src` paths that moved), `scripts/verify.mjs` `RUST_INPUTS` if an entry names a crate that no longer owns the input

- [ ] **Step 1: Confirm the guard is the only enforcement left.** `git grep -n 'crate::local' crates/provider/src` prints nothing; `cargo nextest run -p daemon --test dependencies` passes.
- [ ] **Step 2: Update the documents.** The AGENTS.md bullet becomes: "Keep provider behavior tests in the crate that owns the behavior: an adapter's tests in its `crates/provider-<adapter>`, runtime tests in `crates/provider`, tests that need several real adapters in `crates/provider-builtin`." Run `npm run check:docs`.
- [ ] **Step 3: Final measurement** in the PR description: the family test count against the Task 1 baseline; the Why table re-measured; the `--plan --scope closure --base HEAD` output for an uncommitted one-line edit in `provider-pi`, `provider-codex`, `provider` and `provider-core`. Expected: Pi selects itself, `provider-builtin`, `api` and `daemon`; the other three select the whole family.
- [ ] **Step 4: Move this plan** to `docs/archive/plans/` per [the documentation policy](../policy/documentation.md).
- [ ] **Step 5: Commit** as `docs(provider): 更新 provider crate 拆分后的架构说明`.

## Known limits

- A Codex change still selects the whole family, because the runtime's execution suite runs through the Codex fake. Codex is also the most-edited adapter (55 of the 159 commits). Replacing the fake with a scripted `AgentClient` in that suite would remove the last dev edge; it is a separate project.
- The runtime's test binary stays the largest in the family (about 49k lines with tests). Splitting `skills` or `storage` out is possible later and is not planned here.
- `scripts/check-crate-coverage.py` reports per crate. `provider-core` and `provider-builtin` are exercised mostly from other crates' tests and can read lower than the old combined figure in a per-crate run; a workspace-wide run is unaffected.
- Thirteen PRs in the most-edited area of the repository will conflict with provider branches in flight. The scripted rewrites in each PR description are the mitigation; there is no way to avoid the rebase.
