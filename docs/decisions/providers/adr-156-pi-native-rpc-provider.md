# ADR-156: Pi native RPC provider

- Status: Accepted (implemented).
- Date: 2026-10-17.
- Scope: `crates/provider`, `packages/protocol`; follows [ADR-089](adr-089-provider-owned-composition.md).
- Amended by: [ADR-189](adr-189-pi-without-host-mode.md) for host modes.
- Amended by: [ADR-185](adr-185-native-provider-failure-details.md) for native failure details.

## Context

Pi offers a language-independent JSONL RPC process interface. Ait already has a Pi icon and
native resume command, but had no Rust adapter registered under the `pi` provider identity.
Older Pi RPC releases lack the session settlement event needed to distinguish a completed
run from automatic retries and compaction recovery.

## Decision

- Implement `AgentClient` and `AgentSession` in `provider::local::pi`. Register through
  `Providers`, using `pi` on PATH or the exact `AIT_SERVER_PI_BIN` override. No RPC/wire
  protocol changes.
- Require Pi 1.2 or newer and use `pi --mode rpc`. Pi 1.2.0 is the verified baseline;
  accepting newer versions is not a claim of testing every release. Do not fall back to
  older completion semantics or replay uncertain prompt submissions.
- Query models through `get_available_models` in an ephemeral `--no-session` process.
  Expose qualified `provider/model` IDs to avoid collisions between Pi's model backends.
  Registration itself does not start processes or create storage.
- Create stable UUID sessions with `--session-id`; retain native session file handles for
  subsequent interactive resume. Resolve the host-owned session directory and pass it with
  `--session-dir` as specified by [ADR-174](adr-174-pi-session-import.md), keeping writers and
  discovery aligned even when a per-Agent child environment overrides `HOME`. Validate provider,
  identity and working directory and reject missing history after input was accepted. Pi owns
  credentials, tools, other settings, extensions and native session files. Ait does not rewrite
  those files.
- Correlate command responses by ID. Prompt acceptance and `agent_end` are not completion;
  wait for `agent_settled`. A `handled` acknowledgement is followed by a `get_state`
  ordering barrier: consume already-emitted run events, retain ownership while a run is
  open, and only complete immediately when no run started and native state is idle.
  Compaction-only handled work waits for its end and an idle state barrier, with a separate
  120-second settlement limit.
  Before a new prompt, another state barrier drains old notifications and rejects busy
  native work. Idle notifications do not fail the session. Cancellation sends `abort` and consumes settlement before
  admitting another turn. Process/transport failures terminate uncertain work without replay.
- Project text, reasoning, tool execution and native usage into the existing timeline.
  Apply [ADR-150](adr-150-provider-compaction-lifecycle.md) to Pi's `compaction_start` /
  `compaction_end`, including manual compaction while idle, cancellation/failure, and
  unfinished operations on stream failure or terminal settlement. Each start/end pair shares an adapter-owned UUID;
  native Pi does not supply a compaction ID. Duplicate starts/ends do not duplicate markers.
- Apply [ADR-144](adr-144-provider-error-reporting.md): bounded stderr classification,
  safe structured diagnostics for spawn/version/write/response/frame failures. Preserve bounded,
  redacted native failure details for user-facing errors as specified by
  [ADR-185](adr-185-native-provider-failure-details.md); logs contain only safe structured facts.
  Availability and diagnostic inspection first check whether the configured executable is
  runnable. A missing or non-executable installation returns the existing unavailable result
  without launching a version probe or logging an expected absence, even after a version was
  cached. Recheck on later inspection so installation or permission repair can recover.
  Actual version-probe and session failures retain structured diagnostics.
- Preserve the host's persisted timeline across resume; do not advertise automatic native history
  replay, rewind, steering, auxiliary generation or dynamic thinking controls. Native session
  import and explicit refresh are added by [ADR-174](adr-174-pi-session-import.md).
- Preserve native configuration. Do not expose host modes, as specified by
  [ADR-189](adr-189-pi-without-host-mode.md). Cancel
  extension dialogs explicitly; unknown UI methods fail immediately with an
  `extension_method` protocol diagnostic;
  never implicitly approve them or leave Pi blocked waiting for an unsupported UI.
- Cache a successfully parsed version for each configured client and its clones; failures
  are retried and an environment override gets its own cache. Initial version probing and
  ordinary control operations each have a 10-second budget. Discovery still starts an
  ephemeral RPC process; session creation starts its own writer. Catalog discovery uses
  the existing 30-second total catalog budget. There is no additional process pool.
- Prompt admission has a separate 120-second response budget; events are consumed during
  admission. A timeout is uncertain, closes the process and never replays the prompt.
  Foreground inference after admission has no host timeout. Abort settlement is bounded
  to 10 seconds. Own readers and child process groups through shutdown and drop.
- Outbound records are limited to 64 MiB and inbound records to 2 MiB including LF;
  oversize input is rejected and oversize native frames produce a `frame_limit` protocol
  diagnostic. The transport retains at most 128 intervening records / 8 MiB, accounting
  original wire sizes once. Display events are capped at 256 with reserved cleanup slots.
  Undrainable admission uses a separate 4096-event / 64 MiB display-payload budget;
  afterward the normal 8 MiB budget applies once the backlog fits. Payload accounting
  combines cumulative text with currently queued tool previews. Tool progress always
  replaces unpublished snapshots by key; final items replace pending progress, and
  publishing releases the queued preview bytes. Thus long tool-heavy turns have no
  cumulative tool-output ceiling. Preview sizes are captured during rendering, without
  an extra whole-item serialization (UTF-8 display bytes for strings, rendered JSON bytes
  for structured previews). Admission merges text deltas into chunks of at most 96 KiB.
  Keep the admission event and payload allowances until the backlog fits the normal limits,
  including during cancellation. Active tools are capped at 128, individual text at 96 KiB,
  and tool previews at 32 KiB plus a truncation marker. Preview and process-cleanup helpers
  remain private to Pi; existing adapters retain their own implementations. Excessive distinct queued items, retained previews, or cumulative
  text remain accepted product constraints: they fail the host turn and close Pi, even if
  an extension has completed its native work before acknowledging the command. Queue
  metadata, cached active-tool inputs, and reserved terminal cleanup add overhead beyond
  the payload budget.
- Text projection obeys the storage prefix contract in
  `crates/provider/src/storage/timeline/progress.rs` (`completion` / `suffix`). Multiple
  `Progress` observations may share one native key; their text is additive in publication
  order. Coalescing appends only to the newest queued chunk for that key, and starts another
  chunk when the cap would be exceeded. Pi's `contentIndex` must remain the same index in
  `message_end.content`, and each final block must start with the concatenation of its
  already-published deltas. A changed prefix is rejected by storage as
  `IdempotencyConflict`; the adapter does not silently reorder or rewrite published text.
  Completion during admission removes only unpublished progress for the same key, so that
  progress contributes no stored prefix. Chunking bounds queue entries; it does not lift
  the final block or aggregate assistant-response text cap.
- The 96 KiB text cap follows OpenCode's metadata-headroom policy. Storage's
  `MAX_ENTRY_BYTES` is 768 KiB for the entire serialized `NativeItem`, not just text.
  JSON escaping can expand a control byte to six bytes: the former 128 KiB cap
  (`768 KiB / 6`) exhausted the whole allowance before metadata. At 96 KiB, worst-case
  text occupies 576 KiB, leaving 192 KiB for keys, turn identity, timestamps and JSON
  structure. Storage-backed regression tests cover escaped text and ordered completion.
- Do not pass `--approve` or `--no-approve`, and do not write Pi's trust store. Pi 1.2 honors
  remembered trust, its default trust setting and native trust hooks; an undecided project
  cannot prompt during initial RPC startup and therefore does not load trust-gated `.pi/`
  settings, extensions, skills or packages. Users can establish project trust through Pi's
  interactive CLI. This is distinct from cancellation of later extension UI dialogs.

## Consequences

Pi can be selected and used through the existing Agent workflow without adding a new service
boundary. Unsupported host configuration and structured output are rejected before prompt
submission. Richer extension dialogs, thinking controls, and
additional provider-specific slash-command controls need separate implementations.

Operations and current limitations: [Pi](../../operations/pi.md).
Validation results and scope: [Pi native RPC provider validation](../../reports/providers/pi.md).
