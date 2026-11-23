# Harness Usage Ledger Plan

> **Status:** implemented 2026-11-11 and archived. The current behavior is in [Provider spec](../../architecture/provider.md#harness-用量账本), [daemon operations](../../operations/daemon.md#用量账本) and ADR-246. Deferred: the price-table cost estimate.

**Goal:** Every daemon keeps a durable ledger of the provider ("harness") calls it makes. For each call it records the token usage **per model request**, so a user can see call counts, request counts and tokens per host, provider, model, source and day. The clients add the hosts together.

**Architecture:**

- Adapters that can see request boundaries emit a new `AgentTurnEvent::RequestUsage` with normalized token buckets.
- `AgentManager` already handles the start and end of every turn. It writes one call row per turn and one request row per segment through a `UsageLedger` port.
- A SQLite adapter (`agents/usage.sqlite3`) inserts request rows idempotently and updates daily rollups in the same transaction.
- A read-only RPC returns aggregates, and the clients add hosts together the way the task board does.
- The existing `AgentUsage` snapshot and the context meter do not change (ADR-052).

**Design reference:** [Cindy](https://github.com/makecindy/cindy), mainly `packages/maker-core/src/agents/shared/usage-tracker.ts`, `apps/desktop/src/main/usage/modelUsageDelta.ts` and `apps/desktop/src/main/localDb/worker/opHandlers/piUsageTx.ts`. Like Ait, Cindy drives external agent runtimes (the Claude Agent SDK, the Codex app-server, Pi) instead of calling LLM APIs itself, so most of its design applies here directly.

## Prior art

### Paseo (upstream `getpaseo/paseo`, main `83f4ef99` and fork commit `2c8e8a82`)

Paseo has **no** persistent usage counting, at the fork commit or on current main, and no telemetry SDK. Its two usage features are both already ported to Ait:

| Paseo feature                                                                                       | Kind                                                                 | Ait equivalent today                                                                                        |
| --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Quota "Usage" panel (`usage.list_reports.*`, legacy `provider.usage.list.*`, `UsageSourceRegistry`) | Live provider quota/plan snapshot, 5-minute memory cache, no history | `provider.usage.list.request`, `AgentManager::usage`, `apps/mobile/src/usage/*` (ADR-119, ADR-167, ADR-219) |
| Per-agent `lastUsage` (`usage_updated`, `turn_completed.usage`)                                     | Replace-only token, context and cost snapshot per agent              | `AgentUsage`, `runtime_info.extra.lastUsage`, context-window meter (ADR-052)                                |

Upstream issues ask only for provider quota and spend (#267, #4892). The new method name must not collide with Paseo's `usage.list_reports.*`.

### Cindy rules adopted

1. **A model request is the unit.** Cindy calls it a "segment". A turn is a list of segments, and turn totals are sums of segments, never a replacement snapshot.
2. **Disjoint buckets.**
   - `input` never includes cache tokens. When a provider reports input including cache (Codex/OpenAI), the adapter subtracts: `input = max(0, input − cache_read − cache_write)`.
   - `reasoning` is a diagnostic subset of `output` and is never added to totals.
3. **Split frames merge by field-wise max** under one segment id. For example, Anthropic sends input at `message_start` and output at `message_delta`. Taking the max per field means an endpoint that repeats a value is not counted twice.
4. **Cumulative counters are diffed, never summed.**
   - A drop in any field means the process restarted. The baseline is reset.
   - The first cumulative snapshot after a daemon or process restart is a baseline only and counts as 0. It is better to record only tokens that can be proved than to absorb the history.
5. **Monotonic cursor dedup.** A frame that carries a running total (the Codex `total.totalTokens`) is accepted only if its total is greater than the last accepted one. Duplicate or out-of-order frames are dropped.
6. **Honesty flags.**
   - Every call records `usage_status`: `exact` (all requests seen), `lower_bound` (some requests or fields missing) or `missing` (no token data).
   - Every request records its `boundary`: `request`, `turn` (the provider gives only one figure for the whole turn) or `cumulative_delta`.
7. **Idempotent receipts.** The request row's primary key is `(agent_id, segment_id)`, inserted with `ON CONFLICT DO NOTHING`. Rollups are incremented only when the insert really happened, in the same transaction, so replay after a restart or resume never double-counts.
8. **No guessed cost.** Only a cost the provider itself reports is stored (`cost_usd_reported`). Price-table estimates are a separate read-time feature (see Decisions), because prices change and should not be fixed at write time.

### Cindy parts not adopted

- **Its own gateway price table and CNY/USD billing routes.** Ait does not route model traffic.
- **The loopback Anthropic proxy it uses for concurrent Claude sub-agents.** It places a proxy in the credential path. If Claude sub-agent frames are not in the stream, the call is marked `lower_bound` instead.
- **The Vertex output-lag detector.** It can be added later if a provider shows the same lag.
- **Local time-zone day keys.** Hosts in different time zones must aggregate, so Ait stores UTC timestamps and UTC day rollups. Clients can query detail rows by local day within the retention window.

## Per-adapter request boundaries (checked against the current source)

| Adapter             | Source today                                                                                                                                                    | Ledger mapping                                                                                                                                                                   | Status                   |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| Claude              | `ClaudeUsage::observe` reads `stream_event` `message_start`/`message_delta` (`local/usage.rs`) but drops sub-agent frames (`parent_tool_use_id`, `isSidechain`) | One segment per Anthropic message id, max-merged. Cache read and creation kept separately; the 1-hour creation subset kept. Sub-agent frames become segments with a `parent` key | `request`                |
| Codex               | `thread/tokenUsage/updated` → `usage::codex` uses `last` only                                                                                                   | One segment per accepted `last`, deduped by the `total.totalTokens` cursor. Input minus cached                                                                                   | `request`                |
| Pi                  | Assistant message `usage` (`input`, `output`, `cacheRead`, `cost.total`) in `pi/streaming.rs`                                                                   | One segment per assistant message. Provider cost recorded                                                                                                                        | `request`                |
| OMP                 | Assistant message `usage`, summed into the snapshot by `add_usage` (`omp/stream.rs`)                                                                            | One segment per assistant message, emitted before summing                                                                                                                        | `request`                |
| MiniMax             | `result.usage` per turn (`minimax/session.rs`)                                                                                                                  | One segment per turn                                                                                                                                                             | `turn`                   |
| Antigravity         | Cumulative `result.usage` (`antigravity/streaming.rs`)                                                                                                          | Diffed by the shared cumulative helper; first value after a restart is a baseline                                                                                                | `cumulative_delta`       |
| OpenCode/ACP        | `usage_update` gives context size and cost only                                                                                                                 | No tokens. Cost is diffed if cumulative (to verify per CLI version)                                                                                                              | `missing`                |
| DeepSeek            | Context occupancy only (`deepseek_harness/native/usage.rs`)                                                                                                     | No tokens                                                                                                                                                                        | `missing`                |
| Summary/title calls | `AgentClient::generate_summary` returns text only                                                                                                               | Call counted. Tokens only where the adapter can return them                                                                                                                      | `missing` until extended |

## Design

### What counts as a call

There is one call row per provider turn, plus one per direct client call. Requests are the segments inside a call.

| Source                         | Hook                                                                                                          |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| `user`                         | `AgentManager::send_input` after `session.start_input` accepts (RPC, voice, queued delivery, steer-when-idle) |
| `bot`, `schedule`, `heartbeat` | Same path; the source is taken from the agent record (`internal`, `owner`, labels)                            |
| `autonomous`                   | `streaming::persist_started` for native turns that skip `send_input` (Codex goals)                            |
| `summary`                      | `service/summary_generation.rs` around `AgentClient::generate_summary`                                        |
| rejected start                 | A `start_input` error is recorded with outcome `rejected` and no requests                                     |

These are not new calls: steering into a running turn (its requests attach to the active call), `out_of_band` commands, quota reads (`provider.usage.list`), and history or resume reads.

### Port types (in `crates/provider`, next to the other execution ports, ADR-205)

```rust
/// One model request inside a provider turn, in disjoint token buckets.
pub(crate) struct RequestUsage {
    pub(crate) segment_id: String,        // stable per provider request; the idempotency key
    pub(crate) parent: Option<String>,    // sub-agent stream key, if any
    pub(crate) model: Option<String>,
    pub(crate) boundary: UsageBoundary,   // Request | Turn | CumulativeDelta
    pub(crate) input_tokens: u64,         // excludes cache read and cache write
    pub(crate) output_tokens: u64,        // includes reasoning
    pub(crate) cache_read_tokens: u64,
    pub(crate) cache_write_tokens: u64,
    pub(crate) cache_write_1h_tokens: Option<u64>, // subset of cache_write
    pub(crate) reasoning_tokens: Option<u64>,      // subset of output; never added again
    pub(crate) cost_usd_reported: Option<f64>,     // only when the provider reports it
    pub(crate) complete: bool,            // the terminal frame covered every bucket
}
```

- A new variant `AgentTurnEvent::RequestUsage(RequestUsage)` is added beside the existing `Usage(AgentUsage)`. Adapters keep publishing the snapshot for the meter; the ledger reads only `RequestUsage`.
- A shared `local/usage/cumulative.rs` holds the cumulative-to-delta helper: reset on a drop, baseline after a restart. It is used by Antigravity and any adapter that only has running totals.
- The `UsageLedger` port has the methods `begin_call`, `record_request`, `finish_call`, `recover_interrupted` and `summarize(query)`. Ledger errors never fail a turn: they are logged with `tracing::warn!`.
- Writes go through a bounded channel to one blocking writer task, so the `AgentManager` poll loop never waits on SQLite.

### Storage (`agents/usage.sqlite3`, opened through `provider::storage::open` with its own family id)

```text
calls(call_id PK, agent_id, provider, model, source, outcome, usage_status,
      started_at, ended_at, workspace_id)
      -- outcome: running | completed | failed | cancelled | rejected | interrupted
requests(agent_id, segment_id, call_id, parent, model, boundary,
         input, output, cache_read, cache_write, cache_write_1h, reasoning,
         cost_usd_reported, complete, recorded_at,
         PRIMARY KEY (agent_id, segment_id))
daily_usage(day, provider, model, source,
            calls, failed_calls, requests, input, output, cache_read, cache_write,
            cost_usd_reported, cost_reported_requests, lower_bound_calls,
            PRIMARY KEY (day, provider, model, source))
```

- A call row is inserted as `running` at start and finished in the terminal branch of `AgentManager::poll`, next to `publish_terminal`. At startup, any leftover `running` row becomes `interrupted`.
- A request insert and its rollup update share one transaction. The rollup is updated only when the insert changed a row.
- A request is attributed to the model it reports. If it reports none, the call's `runtime_info.model` is used, falling back to `config.model`.
- **Retention:** `calls` and `requests` are kept 90 days; `daily_usage` is kept indefinitely.
- **Privacy:** no prompt or output text, paths or credentials are stored. Data stays on the host and is not published to the Hub outbox.

### Protocol

- Request `provider.usage.ledger.request { from, to, groupBy: ("provider"|"model"|"source"|"outcome"|"day")[] }`.
- Response `{ rows: { key, calls, failedCalls, requests, inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, reportedCostUsd, costReportedRequests, lowerBoundCalls, missingCalls }[], since }`.
- `since` is when this host's ledger started, so clients can show that earlier history does not exist.
- Clients detect support by method-name negotiation (`features.providerUsageLedger`), the same way `providerUsageList` is detected.
- Schemas go in `packages/protocol/src/messages.ts`; the client method `DaemonClient.listUsageLedger` goes in `packages/client`.

### Clients

- The existing Usage modal gets a **History** tab. It offers 24h, 7d and 30d ranges and tables by provider, model and day: calls, requests, tokens by bucket, cache hit rate, and reported cost where available.
- Figures that are only lower bounds get a "≥" marker and a tooltip explaining why.
- An **All hosts** view is built like `task-board/use-task-board.ts`: it fans out over `useHosts()`, sums rows, and lists hosts that could not be reached instead of counting them as zero.
- The host settings page shows that host's totals.

## Decisions (accepted 2026-11-11)

1. **Claude sub-agents:** counted as segments with a `parent` key when the stream carries their frames; when it does not, the call is `lower_bound`. No proxy.
2. **Summary and title generation:** counted as source `summary`.
3. **Across daemons:** each host counts only its own calls; clients add hosts together. Nothing goes to the Hub outbox.
4. **Cost:** only provider-reported cost is stored. Price-table estimates are a follow-up.
5. **Retention:** `calls` and `requests` 90 days; `daily_usage` indefinitely. No on/off setting in v1 (no content is stored and the data never leaves the host).

## Global Constraints

- Read [`docs/policy/rust.md`](../../policy/rust.md) before any Rust change. `unsafe_code = "forbid"`, `missing_docs`, `unreachable_pub` and `unused_crate_dependencies` are denied by `cargo clippy ... -D warnings`.
- A library crate exposes only items another workspace crate uses ([ADR-116](../../decisions/daemon/adr-116-crate-visibility.md)); no forwarding `pub use` ([ADR-107](../../decisions/daemon/adr-107-direct-imports-from-owning-crates.md)). Dependencies point inward: adapters implement ports.
- Provider behavior tests stay in `crates/provider`, next to the owning adapter or service ([ADR-179](../../decisions/providers/adr-179-provider-owned-tests.md)).
- The existing `AgentTurnEvent::Usage(AgentUsage)` snapshot, `lastUsage`, `usage_updated` and the context meter keep their current behavior and wire format.
- The ledger never fails or delays a turn: write errors are logged with `tracing::warn!` and dropped; SQLite runs on one blocking writer behind a bounded channel.
- Token buckets are disjoint: `input` excludes cache read and cache write; `reasoning` and `cache_write_1h` are subsets and never added to totals.
- `segment_id` is stable for the same native event (so a replayed frame produces the same id) and unique within an agent; include the provider and the turn id in it.
- No prompt or output text, file paths or credentials in the ledger.
- Verification for every task: `npm run verify -- --scope closure` (format, lint, affected tests, docs). Never `--scope all` or full workspace tests (AGENTS.md).
- Before changing a file, run `node scripts/adr.mjs governs <path>`.
- Commit each task on the current branch with a conventional-commit message; do not push.

## Tasks

### Task 1: ADR and provider spec

Write a new ADR under `docs/decisions/providers/` (next unused number; front matter per [the documentation policy](../../policy/documentation.md#adr-元数据), `status: accepted`, `date: 2026-11-11`, `amends: [providers/adr-052-native-provider-capabilities.md]`, `paths: [crates/provider, bins/daemon, packages/protocol, packages/client, apps/mobile]`). It records: the request-usage event and disjoint buckets; the `UsageLedger` port; the `agents/usage.sqlite3` format, idempotency and retention; the `provider.usage.ledger` contract; the five accepted decisions; and that ADR-052 snapshots stay replacement values while request usage is additive. Match the language and structure of neighbouring provider ADRs. Add the conclusions to `docs/architecture/provider.md` (spec states conclusions and cites the ADR). Run `node scripts/adr.mjs index` and `npm run check:docs`. Docs only, no Rust tests.

### Task 2: Request-usage types, ledger port and cumulative helper

In `crates/provider`:

- Add `RequestUsage` and `UsageBoundary` exactly as in "Port types" above (doc comments on every field), with a validity check like `AgentUsage::is_valid` (counters at most 2^53−1, cost finite and ≥ 0, subsets not larger than their parent bucket).
- Add `AgentTurnEvent::RequestUsage(RequestUsage)` in `ports/agent_session.rs`. Every existing `match` on `AgentTurnEvent` must compile; until Task 6, `AgentManager` ignores the new variant.
- Add the `UsageLedger` port (`ports/usage_ledger.rs`) with the record types it takes: call begin (call id, agent id, provider, model, source, workspace id, started at), request (call id + `RequestUsage`), call finish (outcome, ended at), `recover_interrupted`, and `summarize(query) -> rows` with the grouping and row fields of the Protocol section. Source and outcome are enums. Include a no-op implementation for tests and hosts without a ledger.
- Add `local/usage/cumulative.rs` (the existing `local/usage.rs` becomes `local/usage.rs` + `local/usage/` submodules, or follow whatever module layout the crate uses): a per-stream helper that turns cumulative bucket counters into deltas. Rules: the first observation after construction is a baseline and yields no delta; a drop in any field resets the baseline and yields no delta; equal values yield no delta.
- Unit tests for validity and the cumulative helper.

### Task 3: SQLite ledger adapter

Add `storage::usage_ledger::SqliteUsageLedger` implementing `UsageLedger`, opened through `provider::storage::open` with its own `application_id` family constant and `user_version` 1, schema exactly as in "Storage" above plus indexes needed by the summary query (`calls(started_at)`, `requests(call_id)`). Follow `storage/timeline.rs` for pragmas (WAL) and file safety. Behavior:

- `record_request` inserts with `ON CONFLICT DO NOTHING` and updates `daily_usage` in the same transaction only when a row was inserted. The rollup row is keyed by the UTC day of `recorded_at`, the request's model (falling back to the call's model), the call's provider and source.
- `begin_call` inserts `running` and increments `daily_usage.calls`; `finish_call` sets outcome, `ended_at` and `usage_status`, and increments `failed_calls`/`lower_bound_calls` as applicable. `usage_status` is `exact` when every request of the call has `boundary = Request` and `complete = true` and at least one exists; `missing` when there are none; otherwise `lower_bound`. Provider-level "no tokens available" is passed by the caller (Task 6) so `missing` is distinguishable.
- `recover_interrupted` marks every `running` row `interrupted` and returns how many.
- Pruning deletes `calls`/`requests` older than 90 days; run it at open and at most once per day afterwards.
- `summarize` reads `daily_usage` for whole UTC days and the detail tables for partial days, so 24h ranges are exact.
- Writes happen on one blocking thread fed by a bounded channel; a full channel drops the write with `tracing::warn!`.
- Tests: idempotent replay, rollup changes only on a real insert, `usage_status` derivation, interrupted recovery, pruning, summary grouping across a day boundary, and refusal of a foreign or corrupt file.

### Task 4: Claude and Codex request usage

- **Claude** (`local/usage.rs` `ClaudeUsage`, `local/claude/session.rs`): emit one `RequestUsage` per Anthropic message id from `message_start`/`message_delta` stream events, merging split frames by field-wise max; `input` = `input_tokens`, `cache_read` = `cache_read_input_tokens`, `cache_write` = `cache_creation_input_tokens`, `cache_write_1h` = `cache_creation.ephemeral_1h_input_tokens`; `complete` when `message_delta` arrived. Frames with `parent_tool_use_id` / `isSidechain` still do not touch the snapshot but do emit segments with `parent` set. Emit the segment when the next message starts or the turn ends, so the merged value is final. If the stream carries a `result` but no per-message frames, emit one `Turn` segment from `result.usage` minus what was already attributed (never negative).
- **Codex** (`local/usage.rs` `codex`, `local/codex/commands.rs`): on `thread/tokenUsage/updated`, emit a `Request` segment from `last` with `input = max(0, inputTokens − cachedInputTokens)`, `cache_read = cachedInputTokens`, `reasoning = reasoningOutputTokens` if present; accept only when `total.totalTokens` is strictly greater than the last accepted value for that thread, else drop. `segment_id` uses the turn id and the accepted total.
- Fixture tests in each adapter's test module: bucket mapping, split-frame merge, sub-agent parent, duplicate or out-of-order frames not counted, snapshot behavior unchanged.

### Task 5: Pi, OMP, MiniMax, Antigravity, OpenCode/ACP and DeepSeek request usage

- **Pi** (`local/pi/streaming.rs`) and **OMP** (`local/omp/stream.rs`): one `Request` segment per assistant message with usage (`input`, `output`, `cacheRead`, `cacheWrite` if present, `cost.total` as reported cost); aborted all-zero usage emits nothing. OMP keeps summing its snapshot as today.
- **MiniMax** (`local/minimax/session.rs`): one `Turn` segment from `result.usage`.
- **Antigravity** (`local/antigravity/streaming.rs`): feed cumulative `result.usage` through the Task 2 helper; emit `CumulativeDelta` segments only for positive deltas.
- **OpenCode, generic ACP, DeepSeek:** no token segments. Expose the "no token usage" capability so Task 6 can mark calls `missing` (a method on the session or client port, default `true` = tokens available only for adapters that emit segments; choose the smallest change that fits the existing capability pattern).
- Fixture tests in each adapter's module.

### Task 6: AgentManager ledger hooks

In `service/agent_manager*` and `service/summary_generation.rs`:

- `AgentManager::with_usage_ledger(...)`; default is the no-op ledger.
- Add `turn_started_at` to `LiveAgent`. `begin_call` after `session.start_input` accepts in `send_input`, and in `streaming::persist_started` for autonomous turns; a rejected start records a call with outcome `rejected` and ends it immediately.
- Source: `user` unless the agent record marks it as created by a bot, schedule or heartbeat (inspect `internal`, `owner` and labels to find the existing discriminator), or `autonomous` for `persist_started`.
- In the drain loop, forward `RequestUsage` events to `record_request` with the active call id; steering keeps the active call.
- In the terminal branch of `poll` (next to `publish_terminal`), `finish_call` with `completed` / `failed` / `cancelled`, passing whether the provider has token usage.
- `generate_summary` calls in `service/summary_generation.rs` record a `summary` call (begin and finish around the call, outcome by result).
- Service tests with the Codex fake: one call per turn; requests attach to the active call including during a steer; outcome per terminal; rejected start; autonomous turn; summary call; ledger write failure does not fail the turn.

### Task 7: `provider.usage.ledger` RPC, protocol and client SDK

- Rust: dispatch catalog entry and handler for `provider.usage.ledger.request` → `provider.usage.ledger.response`, calling `AgentManager` → `UsageLedger::summarize`; add the method to the list in `crates/metadata` so clients negotiate it. Validate `from < to`, range at most 400 days, `groupBy` non-empty and without duplicates.
- `packages/protocol/src/messages.ts`: request and response schemas from the Protocol section. `packages/client`: `features.providerUsageLedger` from method negotiation and `DaemonClient.listUsageLedger(...)`.
- Tests: routing and validation in Rust; schema round-trip and feature detection in TS.

### Task 8: Daemon wiring

In `bins/daemon/src/host.rs` `compose_provider`: open `agents/usage.sqlite3` under the data directory, call `recover_interrupted` at startup, and pass it with `.with_usage_ledger(...)`. If the ledger cannot be opened, log `tracing::error!` and run with the no-op ledger. Daemon integration test: a restart marks open calls `interrupted` and replayed history does not double-count.

### Task 9: Mobile and desktop History view

In `apps/mobile/src/usage/`: a History tab in the Usage modal (24h / 7d / 30d; tables by provider, model and day with calls, requests, token buckets, cache hit rate = cache_read / (input + cache_read), reported cost); a "≥" marker with explanation for lower-bound or missing rows; an All-hosts view modeled on `task-board/use-task-board.ts` that lists unreachable hosts instead of counting them as zero; a section on the host settings page with that host's totals. Hide everything when the host lacks `features.providerUsageLedger`. Unit tests for cross-host aggregation and formatting. Run the mobile typecheck and affected tests.

### Task 10: Operations docs, plan index and changelog

Add a "Usage ledger" section to `docs/operations/daemon.md` (data file, retention, privacy, honesty flags, per-adapter precision table), a CHANGELOG entry under the unreleased section, and mark this plan done in `docs/plans/README.md` (move it to the archive per the documentation policy). Run `npm run check:docs`.
