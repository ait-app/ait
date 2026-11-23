# Provider Crate Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the single `provider` crate (about 132k lines, about 1,800 tests) into a contract crate, a shared native toolkit, one crate per adapter, the runtime, and a composition crate, so that Cargo enforces the adapter/runtime boundary and `npm run verify` selects tests per adapter.

**Architecture:** Adapters and the runtime both depend on a small contract crate and never on each other in production. The runtime keeps the name `provider`, so `bot`, `api` and `daemon` imports stay as they are; only the composition entry points (`Providers`, `omp_acp`) move to a new top crate. Every file moves once, with `git mv`, and each task leaves the workspace green and mergeable on its own.

**Tech Stack:** Rust 2024 Cargo workspace, `cargo nextest`, [`scripts/verify.mjs`](../../scripts/verify.mjs), the dependency guard in [`bins/daemon/tests/dependencies.rs`](../../bins/daemon/tests/dependencies.rs).

**Spec:** None exists. The Design section below is the specification; Task 1 records it as an ADR.

**Status (2026-11-11):** [ADR-249](../decisions/providers/adr-249-omp-adapter-crate.md) extracted `provider-omp` ahead of this plan, and the daemon appends OMP with `Providers::with`. [ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md) then removed the OMP ACP bridge: `provider::omp_acp`, the `daemon omp-acp` subcommand and any standalone `omp-acp` binary no longer exist, so there is no ACP entry point for `provider-builtin` to own. The `test-support` feature exists, and adapter tests reach runtime operations through the `provider::testing` wrappers instead of widened `cfg` gates. Where the tasks below create `provider-omp`, treat that work as done; where they move or export `omp_acp`/`serve_acp`, drop that step.

## Design

Target graph. Solid arrows are normal dependencies, dotted arrows are dev-only.

```
domain <- model <- provider-core <- provider-native <- provider-<adapter> x8
                        ^                                     ^
                        |                                     |
                     provider (runtime)  <----------  provider-builtin
                        ^      ^                              ^
                       bot    api                           daemon

dev-only:  provider-<adapter> ..> provider      (end-to-end adapter tests drive the real runtime)
           provider           ..> provider-codex (the runtime's execution suite drives the Codex fake)
           api                ..> provider-builtin
```

| Crate                                                                                                                                                            | Contents (current path under `crates/provider/src`)                                                                                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `provider-core`                                                                                                                                                  | `ports/{agent_session,account_usage,controls,diagnostics,environment,native_history}`, new `ports/bot_session`, and `protocol/{prompt,timeline,provider,agent_config,usage,resume}` plus whatever those six reference                    |
| `provider-native`                                                                                                                                                | `local/{acp_config,acp_permissions,acp_streaming,acp_transport,compaction,configuration,diagnostics,elicitation,images,metadata_process,notes,process_group,prompt_blocks,sandbox,session_preview,summary_model,tool_detail,usage}`      |
| `provider-codex`, `provider-claude`, `provider-opencode`, `provider-pi`, `provider-omp`, `provider-deepseek-harness`, `provider-antigravity`, `provider-minimax` | `local/<adapter>.rs` and `local/<adapter>/`                                                                                                                                                                                              |
| `provider` (runtime, name unchanged)                                                                                                                             | `service`, `rpc`, `storage`, `dispatch`, `connection`, `capabilities`, `timeline_projection`, `skills`, `support`, `diagnostics`, `installation`, `ports/{agent,mcp_scope}`, `protocol/{agent,agent_lifecycle,controls,native_sessions}` |
| `provider-builtin`                                                                                                                                               | `composition` (`Providers`), `omp_acp`, and the tests that exercise the runtime against more than the Codex adapter                                                                                                                      |

Decisions the executor must not reopen:

1. **The runtime keeps the name `provider`.** Renaming it would touch 42 files in `bot`, `api` and `daemon` for no gain.
2. **No production edge between an adapter and the runtime, in either direction.** Today four edges break this: `ports/agent_session.rs:12`, `local/claude.rs:28` and `local/codex.rs:40` import `service::bot_execution::{BotSessionSupport, InstructionDelivery}`; `rpc/agent_execution/native_sessions.rs:135` and `service/agent_manager/native_sessions.rs:393` compare against `local::pi::PROVIDER`. Task 2 removes them.
3. **Adapter tests stay with the adapter** and reach the runtime through a dev-dependency on `provider` with the `test-support` feature. This amends ADR-116 §3, which gates test entry points with `#[cfg(test)]`; `cfg(test)` does not cross crates.
4. **`provider` may dev-depend on exactly one adapter, `provider-codex`.** The 7.6k-line `service/agent_execution/tests` suite drives the Codex Python fake. Any other adapter in the runtime's dev-dependencies would make every adapter change select the whole family, because `verify.mjs` follows dev edges.
5. **A fixture moves with the crate that owns the fake.** Other crates and the TypeScript clients reference it by path, as the daemon tests do today.
6. **Stop point:** after Task 6 (first adapter extracted) compare the measurements with the Task 1 baseline before starting Task 7.

## Global Constraints

- Read [`docs/policy/rust.md`](../policy/rust.md) before any Rust change; `unsafe_code = "forbid"`, `missing_docs`, `unreachable_pub` and `unused_crate_dependencies` are all denied by `cargo clippy ... -D warnings`.
- A library crate exposes only items another workspace crate uses ([ADR-116](../decisions/daemon/adr-116-crate-visibility.md)). Widen visibility one compiler error at a time; never make a module tree `pub` wholesale. No forwarding `pub use` ([ADR-107](../decisions/daemon/adr-107-direct-imports-from-owning-crates.md)).
- Every moved file moves with `git mv` in a commit that contains no content edits; path rewrites go in the following commit. This keeps `git log --follow` and in-flight branch rebases working.
- No behavior, wire format, persisted format, environment variable or provider id changes. If a test needs editing beyond import paths and visibility, stop and report it.
- Test count parity: the number of tests listed for the provider family never drops. A drop means a `mod tests;` line was lost in a move.
- New crates live at `crates/<name>`; the workspace `members = ["bins/daemon", "crates/*"]` picks them up. Add each to `[workspace.dependencies]` in the root `Cargo.toml`.
- Verification for every task: `npm run verify -- --scope closure`. Do not use `--scope all`; AGENTS.md reserves the full workspace suite for an explicit user request.
- macOS-local green is not sufficient for Tasks 4 and 7: `sandbox` and several adapters have `cfg(target_os = "linux")` code. Wait for the CI Rust job before merging.

## Review Focus

1. A `mod tests;` declaration lost in a move silently removes tests. Pinned by the test-count step in Tasks 3–7.
2. `test-support` enabled in a release build would ship test entry points. Pinned by the guard rule in Task 1 and the `cargo tree` step in Task 5.
3. A fixture path that still points at `crates/provider/tests/fixtures/` fails only at test run time, and for TypeScript only in the UI job. Pinned by the stale-path step in Tasks 4, 6 and 7.
4. An item gated `#[cfg(test)]` in one crate and called from another fails to compile only in the test profile. Pinned by running `cargo clippy --all-targets` and a featureless `cargo clippy -p provider --lib` in Tasks 5–7.
5. A new production edge from the runtime to an adapter added by an unrelated PR while the split is in flight. Pinned by the source-scan test added in Task 2 and removed in Task 8.

---

### Task 1: ADR, baseline and guard rules

**Files:**

- Create: `docs/decisions/providers/adr-228-provider-crate-family.md` (use the next free number on `main` at merge time; numbers already collide)
- Modify: `docs/decisions/README.md` (index entry), `bins/daemon/tests/dependencies.rs`

**Interfaces:**

- Produces: guard allow-lists for every crate name in the Design table, and a `dev_only` rule later tasks rely on.

- [ ] **Step 1: Record the baseline** in the PR description: `cargo nextest list -p provider --message-format oneline | wc -l`, the wall time of `cargo nextest run -p provider` on a warm build, and the wall time of the same command after `touch crates/provider/src/local/minimax.rs`.

- [ ] **Step 2: Write the failing guard test** in `bins/daemon/tests/dependencies.rs`:

```rust
#[test]
fn provider_family_keeps_adapters_and_runtime_apart() {
    let package = |name: &str, dependencies: Value| {
        json!({"id": name, "name": name, "dependencies": dependencies})
    };
    let edge = |name: &str, kind: Value| json!({"name": name, "path": "x", "kind": kind});
    let packages = vec![
        package("provider-core", json!([])),
        package(
            "provider",
            json!([
                edge("provider-core", Value::Null),
                edge("provider-codex", json!("dev")),
                edge("provider-claude", json!("dev")),
            ]),
        ),
        package("provider-codex", json!([edge("provider", json!("dev"))])),
        package("provider-claude", json!([edge("provider", Value::Null)])),
        package(
            "provider-pi",
            json!([{"name": "provider", "path": "x", "features": ["test-support"]}]),
        ),
    ];
    assert_eq!(
        violations(&packages),
        [
            "provider -> provider-claude",
            "provider-claude -> provider outside tests",
            "provider-pi -> provider outside tests",
        ]
    );
    let runtime = vec![
        package("provider", json!([edge("provider-codex", Value::Null)])),
        package("provider-codex", json!([])),
    ];
    assert_eq!(violations(&runtime), ["provider -> provider-codex outside tests"]);
}
```

- [ ] **Step 3: Run it.** `cargo nextest run -p daemon --test dependencies` — expected: FAIL with `unregistered workspace package: provider-core`.

- [ ] **Step 4: Implement the rules** in `violations`. Allow-lists:

| Package                   | Allowed workspace dependencies                                                    | Must be `kind == "dev"`         |
| ------------------------- | --------------------------------------------------------------------------------- | ------------------------------- |
| `provider-core`           | `domain`, `model`                                                                 | —                               |
| `provider-native`         | `domain`, `model`, `provider-core`                                                | —                               |
| each `provider-<adapter>` | `domain`, `model`, `provider-core`, `provider-native`, `provider`, `persistence`  | `provider`, `persistence`       |
| `provider`                | `domain`, `model`, `persistence`, `provider-core`, `provider-codex`               | `persistence`, `provider-codex` |
| `provider-builtin`        | `domain`, `model`, `persistence`, `provider-core`, `provider`, all eight adapters | `persistence`                   |
| `bot`, `api`, `daemon`    | existing list plus `provider-core`; `api` and `daemon` also `provider-builtin`    | `api`: `provider-builtin`       |

Replace the existing hard-coded `persistence outside tests` branch with this per-package dev-only list so there is one mechanism.

- [ ] **Step 5: Run it.** Same command — expected: PASS, including the two existing guard tests.

- [ ] **Step 6: Write the ADR** with the Design section's graph, table and six decisions; status `Accepted`; state that it amends ADR-116 §3 and the test-location sentence of ADR-179. Add the index entry. Run `npm run check:docs`.

- [ ] **Step 7: Commit** as `docs(provider): 记录 provider crate 拆分决策并加入依赖守卫`.

### Task 2: Remove the four cross-layer edges in place

**Files:**

- Create: `crates/provider/src/ports/bot_session.rs`, `crates/provider/src/tests.rs`
- Modify: `crates/provider/src/lib.rs`, `ports.rs`, `ports/agent_session.rs`, `ports/native_history.rs`, `service/bot_execution.rs`, `service/agent_manager.rs`, `service/agent_manager/native_sessions.rs`, `rpc/agent_execution/native_sessions.rs`, `rpc/agent_execution/resume.rs`, `rpc/agent_execution/bot.rs`, `local/claude.rs`, `local/codex.rs`, `local/pi.rs`, and the two external uses of `BotSessionSupport`/`InstructionDelivery` (find with `grep -rn 'BotSessionSupport\|InstructionDelivery' crates/bot crates/api bins`)

**Interfaces:**

- Produces: `ports::bot_session::{BotSessionSupport, InstructionDelivery}` (moved verbatim, same derives and serde attributes); `ports::native_history::CwdLookup`; `AgentClient::cwd_lookup`; `AgentManager::cwd_lookup`.

- [ ] **Step 1: Write the failing boundary test** in `crates/provider/src/tests.rs`, declared from `lib.rs` as `#[cfg(test)] mod tests;`:

```rust
#[test]
fn layers_reference_only_what_their_future_crate_may_depend_on() {
    const RUNTIME: &[&str] = &[
        "service", "rpc", "storage", "dispatch", "connection", "support",
        "timeline_projection", "skills", "capabilities", "composition", "installation",
    ];
    let mut found = Vec::new();
    found.extend(references("local", RUNTIME));
    found.extend(references("ports", &[RUNTIME, &["local"][..]].concat()));
    for runtime in ["service", "rpc", "storage", "dispatch", "connection"] {
        found.extend(references(runtime, &["local", "composition"]));
    }
    assert_eq!(found, Vec::<String>::new());
}
```

`references(root, forbidden)` walks `src/<root>.rs` and `src/<root>/`, skips any path with a `tests` component, `tests.rs` and `test_support*`, skips lines whose trimmed text starts with `//`, and returns `"<relative path>:<line>"` for each line containing `crate::<name>::` for a forbidden name. It must also catch grouped imports: inside a `use crate::{ ... };` block, an entry that starts with `<name>::` or is exactly `<name>` counts (21 production files use the grouped form today; none of them crosses a layer).

- [ ] **Step 2: Run it.** `cargo nextest run -p provider -E 'test(layers_reference_only)'` — expected: FAIL listing exactly the five lines named in Design decision 2.

- [ ] **Step 3: Move the bot session types.** Create `ports/bot_session.rs` with both types cut from `service/bot_execution.rs`, add `pub mod bot_session;` to `ports.rs`, and update every import. Do not leave a re-export behind.

- [ ] **Step 4: Replace the Pi comparisons with a capability.** Add to `ports/native_history.rs`:

```rust
/// Working-directory spelling a provider's native session lookup expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CwdLookup {
    /// The canonical directory Ait stores for the Agent.
    #[default]
    Canonical,
    /// The spelling the client sent, before canonicalization.
    Requested,
}
```

Add `fn cwd_lookup(&self) -> CwdLookup { CwdLookup::Canonical }` to `AgentClient`; `PiClient` overrides it with `Requested`. Add `pub(crate) fn cwd_lookup(&self, provider: &str) -> CwdLookup` to `AgentManager`, returning `Canonical` for an unregistered provider. Change the signatures to `inspection_cwd<'a>(lookup: CwdLookup, requested_cwd: &'a str, canonical_cwd: &'a str) -> &'a str` and `listing_options(lookup: CwdLookup, options: &ListOptions, lookup_cwd: Option<&str>) -> ListOptions`; both rpc callers obtain `lookup` from `self.manager`.

- [ ] **Step 5: Pin the capability.** In `rpc/agent_execution/native_sessions/tests.rs`:

```rust
#[test]
fn inspection_uses_the_spelling_the_provider_asks_for() {
    assert_eq!(inspection_cwd(CwdLookup::Requested, "/a/link", "/a/real"), "/a/link");
    assert_eq!(inspection_cwd(CwdLookup::Canonical, "/a/link", "/a/real"), "/a/real");
}
```

and in the Pi adapter's tests, `assert_eq!(PiClient::new("pi".into()).cwd_lookup(), CwdLookup::Requested);`.

- [ ] **Step 6: Run.** `cargo nextest run -p provider -E 'test(layers_reference_only) | test(inspection_uses) | test(native_sessions)'` — expected: PASS. Then `npm run verify -- --scope closure`.

- [ ] **Step 7: Commit** as `refactor(provider): 移除适配器与运行时之间的生产依赖`.

### Task 3: Extract `provider-core`

**Files:**

- Create: `crates/provider-core/Cargo.toml`, `crates/provider-core/src/lib.rs`
- Move: the `provider-core` row of the Design table into `crates/provider-core/src/{ports,protocol}/`
- Modify: root `Cargo.toml`, `crates/provider/Cargo.toml`, `crates/provider/src/{lib.rs,ports.rs,protocol.rs}`

**Interfaces:**

- Produces: `provider_core::ports::{agent_session, account_usage, bot_session, controls, diagnostics, environment, native_history}` and `provider_core::protocol::{prompt, timeline, provider, agent_config, usage, resume, ...}`, all `pub`. `provider::ports` keeps `agent` and `mcp_scope`; `provider::protocol` keeps `agent`, `agent_lifecycle`, `controls`, `native_sessions`.

- [ ] **Step 1: Create the crate skeleton.** `Cargo.toml` copies the `[package]` and `[lints]` blocks from `crates/provider/Cargo.toml` with `name = "provider-core"`. `lib.rs`:

```rust
//! Contracts shared by the provider runtime and the native adapters.
//!
//! Adapters implement these ports; the runtime drives them. Neither side depends on the other.

pub mod ports;
pub mod protocol;
```

- [ ] **Step 2: Move with `git mv`, commit, no edits.** The protocol set is the six modules named in the table plus their transitive references; today that adds `agent_execution` (through `provider::Details::draft_config`) and whatever it pulls in. Let `cargo check -p provider-core` name the closure. Never move `agent`, `agent_lifecycle`, `controls` or `native_sessions`.

- [ ] **Step 3: Keep old paths resolving for one commit.** In `crates/provider/src/ports.rs` and `protocol.rs` add private aliases such as `pub(crate) use provider_core::ports::{agent_session, account_usage, bot_session, controls, diagnostics, environment, native_history};` so `crate::ports::agent_session::...` still compiles everywhere in `provider`.

- [ ] **Step 4: Make it compile.** Change `pub(crate)` to `pub` only where `cargo check -p provider` reports a privacy error, and add the doc comment `missing_docs` then asks for. Start `provider-core`'s dependency list from `provider`'s and delete entries until `unused_crate_dependencies` is silent in both crates. A test in a moved module that uses `crate::rpc` or `crate::service` stays in `provider`, next to the module it exercises.

- [ ] **Step 5: Rewrite paths and delete the aliases.** In `crates/provider/src`, replace `crate::ports::<moved>` with `provider_core::ports::<moved>` and `crate::protocol::<moved>` with `provider_core::protocol::<moved>`, then remove the Step 3 aliases. Update the Task 2 boundary test: drop the `ports` rule (Cargo now enforces it).

- [ ] **Step 6: Verify.** `npm run verify -- --scope closure` passes. `cargo nextest list -p provider -p provider-core --message-format oneline | wc -l` is at least the Task 1 baseline plus the tests added in Task 2.

- [ ] **Step 7: Commit** as `refactor(provider): 提取 provider-core 契约 crate`.

### Task 4: Extract `provider-native`

**Files:**

- Create: `crates/provider-native/Cargo.toml`, `crates/provider-native/src/lib.rs`
- Move: the eighteen modules in the `provider-native` row to `crates/provider-native/src/`; `crates/provider/tests/fixtures/linux_sandbox_{probe,terminal}.py` to `crates/provider-native/tests/fixtures/`
- Modify: `crates/provider/src/local.rs`, `scripts/verify.mjs`, `scripts/verify.test.mjs`, `.github/workflows/ci.yml` if it still names the fixture directory

**Interfaces:**

- Consumes: `provider_core::{ports, protocol}`.
- Produces: `provider_native::<module>` for each moved module; only items an adapter uses are `pub`.

- [ ] **Step 1: Generalize the shared-fixture rule first.** In `scripts/verify.test.mjs` add a case asserting that `plan(["crates/provider-omp/tests/fixtures/omp_question.json"])` selects the TypeScript packages exactly as the existing `crates/provider/tests/fixtures/omp_question.json` case does. Run `node --test scripts/verify.test.mjs` — expected: FAIL. In `scripts/verify.mjs` replace the `startsWith("crates/provider/tests/fixtures/")` test with `/^crates\/provider(?:-[a-z-]+)?\/tests\/fixtures\//`. Run again — expected: PASS.

- [ ] **Step 2: Skeleton, `git mv`, commit.** `lib.rs` carries the crate doc `//! Process, sandbox, ACP and media helpers shared by native provider adapters.` and one `pub mod` per moved module. `nix` moves here as the `cfg(unix)` dependency.

- [ ] **Step 3: Alias, compile, rewrite, remove alias** exactly as Task 3 Steps 3–5, with `crate::local::<module>` becoming `provider_native::<module>`. The `summary_model` test that constructs a `DeepSeekHarnessClient` stays in `provider` under `local/deepseek_harness` until that adapter moves.

- [ ] **Step 4: Stale-path check.** `grep -rn 'crates/provider/tests/fixtures/linux_sandbox' --include='*.rs' --include='*.ts' --include='*.mjs' --include='*.yml' . | grep -v '^./docs/'` prints nothing.

- [ ] **Step 5: Verify.** `npm run verify -- --scope closure`; test count for `-p provider -p provider-core -p provider-native` is not below the previous task's. Wait for the CI Rust job (Linux sandbox tests).

- [ ] **Step 6: Commit** as `refactor(provider): 提取 provider-native 适配器工具 crate`.

### Task 5: Extract `provider-builtin` and introduce `test-support`

**Files:**

- Create: `crates/provider-builtin/Cargo.toml`, `crates/provider-builtin/src/lib.rs`
- Move: `crates/provider/src/composition.rs`, `composition/`, `omp_acp.rs` to `crates/provider-builtin/src/`
- Modify: `crates/provider/Cargo.toml` (`[features] test-support = []`), `crates/provider/src/{lib.rs,local.rs}`, `bins/daemon/src/{host.rs,main.rs}`, `crates/api/src/tests/mcp/scoped/support.rs`, `bins/daemon/Cargo.toml`, `crates/api/Cargo.toml`

**Interfaces:**

- Consumes: `provider::service::agent_manager::{AgentManager, AgentManagerError}`, `provider::service::summary_generation::Generation`.
- Produces: `provider_builtin::Providers` (same methods: `new`, `configured`, `register`, `summary_generator`) and `provider_builtin::omp_acp::serve(program: PathBuf) -> std::io::Result<()>`. `provider` gains the `test-support` feature: an item another crate's tests call is gated `#[cfg(any(test, feature = "test-support"))]` instead of `#[cfg(test)]`.

- [ ] **Step 1: Move and compile.** Until the adapters leave, `provider` exposes them as `#[doc(hidden)] pub mod local;` with each adapter module and its client type, constructors and builder methods `pub`. These are the same items the adapter crates will export; nothing else in `local` becomes `pub`.

- [ ] **Step 2: Relocate the multi-adapter runtime tests** to `crates/provider-builtin/src/` next to `composition/tests.rs`: the cases in `service/provider_catalog/tests.rs` that build `ClaudeClient`, `OpenCodeClient` or `DeepSeekHarnessClient`, the Pi case in `service/provider_catalog/tests/diagnostics.rs`, and the DSH case in `service/agent_execution/tests/subscriptions/creation.rs`. After this step `grep -rn 'crate::local::' crates/provider/src/{service,rpc,storage}` matches only `local::codex`.

- [ ] **Step 3: Gate what those tests need** with the feature, for example `summary_generation::test_config`. `provider-builtin`'s dev-dependency is `provider = { workspace = true, features = ["test-support"] }`.

- [ ] **Step 4: Extend the boundary test.** Add a second test asserting that test files under `service`, `rpc` and `storage` reference no adapter other than `local::codex`.

- [ ] **Step 5: Verify the feature never ships.** `cargo tree -p daemon -e features,no-dev | grep test-support` prints nothing. `cargo clippy --locked -p provider --lib -- -D warnings` (featureless) passes. `npm run verify -- --scope closure` passes.

- [ ] **Step 6: Commit** as `refactor(provider): 提取 provider-builtin 组装 crate`.

### Task 6: Extract `provider-minimax` (pilot) and measure

**Files:**

- Create: `crates/provider-minimax/Cargo.toml`
- Move: `crates/provider/src/local/minimax.rs` to `crates/provider-minimax/src/lib.rs`; `local/minimax/*` to `crates/provider-minimax/src/`
- Modify: `crates/provider/src/local.rs`, `crates/provider-builtin/{Cargo.toml,src/composition.rs}`, `apps/mobile/e2e/browser/acp-provider-catalog.spec.ts` (fixture path becomes `crates/provider-minimax/src/tests/fixtures/acp.py`)

**Interfaces:**

- Consumes: `provider_core`, `provider_native`; in tests, `provider` with `test-support`.
- Produces: `provider_minimax::MiniMaxClient` with `new(program: PathBuf)` and `with_image_directory(PathBuf)`.

The recipe, reused verbatim by Task 7:

- [ ] **Step 1: `git mv`, commit.**
- [ ] **Step 2: Alias old paths in the new `lib.rs`** for one commit: `pub(crate) use provider_core::{ports, protocol};`, a private `mod local` re-exporting the `provider_native` modules this adapter uses, and `#[cfg(test)] pub(crate) use provider::{service, storage, rpc, timeline_projection};`.
- [ ] **Step 3: Compile production code.** `cargo check -p provider-minimax`. Dependencies: start from `provider`'s list and delete until `unused_crate_dependencies` is silent. `pub(in crate::local::minimax)` becomes `pub(crate)`.
- [ ] **Step 4: Compile tests.** `cargo check -p provider-minimax --tests`. Each privacy error on a `provider` item is resolved by making that item `pub` with a doc comment, and gating it with `test-support` if it was `#[cfg(test)]`. If a test needs `crate::test_support::worker_with_client` (persistence-backed execution), it is a runtime test: move it to `crates/provider/src/service/agent_execution/tests/`.
- [ ] **Step 5: Rewrite paths, remove the aliases**, point `provider-builtin` at `provider_minimax::MiniMaxClient`, and delete the module from `provider::local`.
- [ ] **Step 6: Stale-path check.** `grep -rn 'crates/provider/src/local/minimax' . --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.mjs' | grep -v '^./docs/'` prints nothing.
- [ ] **Step 7: Verify.** `npm run verify -- --scope closure`; featureless `cargo clippy --locked -p provider --lib -- -D warnings`; family test count not below the previous task's.
- [ ] **Step 8: Measure and report in the PR description.** After a one-line edit in `crates/provider-minimax/src/lib.rs`: the output of `npm run verify -- --plan --scope closure` must name `provider-minimax`, `provider-builtin`, `api` and `daemon` and must not name `provider` or any other adapter; record the wall time of `cargo nextest run -p provider-minimax` beside the Task 1 baseline.
- [ ] **Step 9: Commit** as `refactor(provider): 提取 provider-minimax 适配器 crate`.

**Gate:** proceed to Task 7 only if Step 8's selection is as stated. If `provider` appears, a dev edge from the runtime to this adapter survived; fix that before continuing.

### Task 7: Extract the remaining adapters

One PR per adapter, in this order, each following Task 6 Steps 1–9 with the names substituted.

| Order | Crate                       | Dependencies that move with it       | Fixtures that move to `<crate>/tests/fixtures/`                                                                                                          | Notes                                                                                                      |
| ----- | --------------------------- | ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| 1     | `provider-antigravity`      | —                                    | `antigravity_tool_cards.json`                                                                                                                            |                                                                                                            |
| 2     | `provider-omp`              | —                                    | `omp_dialogs.json`, `omp_question.json`                                                                                                                  | Exports `serve_acp`; `provider_builtin::omp_acp` calls it.                                                 |
| 3     | `provider-pi`               | `url`                                | `pi_tool_cards.json`                                                                                                                                     | Largest test tree (10.7k lines).                                                                           |
| 4     | `provider-claude`           | `unicode-normalization` (macOS)      | `claude_code.py`, `claude_keychain.py`                                                                                                                   | `provider-builtin`'s composition test and `bins/daemon/tests/process/voice.rs` reference `claude_code.py`. |
| 5     | `provider-deepseek-harness` | `tokio-tungstenite`, `serde_yaml_ng` | `deepseek_harness_native.sh`, `dsh_metadata.cjs`                                                                                                         | Take the parked `summary_model` test from Task 4.                                                          |
| 6     | `provider-opencode`         | —                                    | `opencode_acp.py`, `opencode_acp_proxy.py`, `opencode_observation.py`, `opencode_subagents.py`, `opencode_tool_cards.json`, `opencode-tool-history.json` | Two daemon tests reference `opencode_acp.py`.                                                              |
| 7     | `provider-codex`            | `webrtc`, `rtc`, `opus2`, `toml`     | `codex_app_server.py`                                                                                                                                    | See below.                                                                                                 |

For each fixture, update every reference found by `grep -rn '<file name>' --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.mjs' . | grep -v '^./docs/'`; the TypeScript consumers are under `packages/protocol/src`, `packages/client/src`, `apps/mobile/src`, `apps/mobile/e2e` and `apps/desktop/e2e`. `legacy_tool_details.json` and the three `paseo-*.json` files stay in `crates/provider/tests/fixtures/`.

**Codex specifics.** `crates/provider/src/test_support.rs` splits in two:

- `crates/provider-codex/src/testing.rs`, declared `#[cfg(any(test, feature = "test-support"))] pub mod testing;`, receives `Fixture` (`new`, `spec`, `client`, `mode`, `requests`) and `codex_history`. `CodexClient::with_fixture_media` takes the same gate. `provider-codex` declares `[features] test-support = []`.
- `crates/provider/src/test_support.rs` keeps `worker_with_client` and `request`, importing `provider_codex::testing::Fixture`. `provider`'s dev-dependency is `provider-codex = { workspace = true, features = ["test-support"] }`.

After this PR `provider/src/local.rs` and `provider::local` no longer exist.

### Task 8: Close out

**Files:**

- Delete: `crates/provider/src/tests.rs` and its `mod tests;` line (Cargo now enforces every rule the scan test held)
- Modify: `docs/architecture/README.md` (crate table and the ADR-179 sentence), `AGENTS.md` line 11, `docs/README.md` (one pointer to the ADR), `docs/plans/README.md` (remove this plan's row), `scripts/verify.mjs` `RUST_INPUTS` if any entry still names a crate that no longer owns the input

- [ ] **Step 1: Confirm the guard is the only enforcement left.** `grep -rn 'crate::local' crates/provider/src` prints nothing; `cargo nextest run -p daemon --test dependencies` passes.
- [ ] **Step 2: Update the documents.** AGENTS.md line 11 becomes: "Keep provider behavior tests in the crate that owns the behavior: an adapter's tests in its `crates/provider-<adapter>`, runtime tests in `crates/provider`." Run `npm run check:docs`.
- [ ] **Step 3: Final measurement** in the PR description: family test count against the Task 1 baseline, and the `--plan --scope closure` output for a one-line edit in `provider-codex`, `provider-pi`, `provider` and `provider-core`. Expected: Pi selects itself, `provider-builtin`, `api`, `daemon`; Codex and `provider` select the whole family; core selects the whole family.
- [ ] **Step 4: Move this plan** to `docs/archive/plans/` per [the documentation policy](../policy/documentation.md).
- [ ] **Step 5: Commit** as `docs(provider): 更新 provider crate 拆分后的架构说明`.

## Known limits

- A Codex adapter change still selects the whole family, because the runtime's execution suite runs through the Codex fake. Moving that suite to `provider-builtin` would remove the last dev edge; do it only if Codex changes prove to dominate CI time.
- `scripts/check-crate-coverage.py` reports per crate. Small crates whose behavior is exercised from another crate's tests (`provider-core`, `provider-builtin`) can read lower than the old combined figure in a per-crate run; a workspace-wide run is unaffected.
- Twelve PRs touching the most-edited area of the repository will conflict with in-flight provider branches. Land each adapter PR within a day of opening it and announce the adapter beforehand.
