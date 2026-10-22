# Pi provider

Ait integrates the native Pi coding agent through `pi --mode rpc`. Install Pi separately and
configure authentication through Pi itself. The verified baseline is
`@earendil-works/pi-coding-agent` 1.2.0; older `@mariozechner/pi-coding-agent` 0.x RPC
versions are rejected because their completion semantics differ.

```sh
pi --version
pi
```

After installing/configuring Pi, refresh Ait's provider catalog and select **Pi**. Models are
listed using qualified IDs such as `anthropic/claude-sonnet-4-6`, taken from the installed
Pi runtime. Ait does not maintain a static Pi model catalog.

The daemon uses `pi` on PATH. To select another executable, set `AIT_SERVER_PI_BIN` to its
path in the daemon's environment before starting it. An explicit missing executable does
not fall back to PATH. No provider process is launched merely by registering the adapter.

## Supported behavior

- Model discovery without creating persistent sessions or submitting prompts.
- Model-specific reasoning controls derived from Pi's `reasoning` and `thinkingLevelMap`
  metadata, with the native configured default model marked when the required state read
  identifies a catalog model.
- Native session creation and resume with stable IDs.
- Native session discovery, import, and explicit history refresh from Pi's JSONL storage.
- Text and attachment input, inline images accepted by the native model, streaming text
  and reasoning, tool progress/results, and native usage snapshots.
- Cancellation and process cleanup, including failure and uncertain submission paths.
- Ait display timeline retention across daemon restart.
- Compaction start/completion/failure markers, including manual compaction while idle.
- Safe failure classification and fixed user-facing explanations, without retaining raw errors.

Pi owns its credentials, session files, tools, extensions and settings. The **Native Tools**
mode uses those settings; it does not add a permission sandbox. Extension select, confirm,
input and editor dialogs are explicitly cancelled. Notification-only extension UI records
(`notify`, `setStatus`, `setWidget`, `setTitle`, `set_editor_text`) are ignored. Unknown
UI methods fail immediately with a protocol diagnostic (`extension_method`). This can cause extensions requiring interaction to decline their operation.

Rewind, live steering, host-provided MCP/tool policies,
custom system prompts, structured output and auxiliary summaries
are not exposed by this adapter. Unsupported settings are rejected instead of ignored.
Existing native history changed outside Ait is not automatically re-imported into the Ait
display timeline; use explicit history refresh.

## Reasoning controls and defaults

Discovery requires `get_available_models` and `get_state`, each under the normal 10-second
control deadline. If Pi rejects, times out, or fails the state read, discovery fails instead of
silently dropping defaults that profile forms persist. A slower successful read returns the same defaults.
A successful state response without model information also leaves the default unmarked.
Validation accepts nonempty, control-free thinking-level IDs of at most 512 bytes and lets Pi
validate or resolve the effective level,
including clamping choices from stale catalogs or imported profiles. Creation, configuration
updates, and restore use the same rule, so a stored level that is no longer offered does not
prevent resume. Explicit model selections still require a model in the current catalog;
selection validation requests only `get_available_models`, without reading native state.

Reasoning models expose `off`, `minimal`, `low`, `medium`, and `high` unless Pi explicitly
disables a level with a null mapping. `xhigh` and `max` appear only when Pi supplies a non-null
mapping. Additional non-null map keys are exposed with the ID as their fallback label, so new
Pi levels remain selectable without an adapter update. Models without reasoning expose no
thinking choices. The catalog reports the current
model's effective `thinkingLevel` from Pi's state as its default only when that level is offered.
Other models and missing or unsupported levels leave the thinking default
unset; the composer preserves an unspecified selection so Pi can apply its saved settings and
per-model defaults. The composer labels this unspecified choice "Default".
Discovery does not guess a level or override Pi's saved settings on startup.
Leaving an unknown thinking default unspecified in profile and agent forms is specific to Pi;
other providers still seed their first offered thinking option when no default is advertised.

Without an explicit thinking selection, creation and resume retain Pi's effective native level.
An explicit selection uses `set_thinking_level`, followed by `get_state` readback so runtime
information reflects native clamping. `runtimeInfo.thinkingOptionId` stays unset so directory
filtering follows the latest stored selection, including pending configuration changes;
`runtimeInfo.extra.thinkingLevel` carries the effective
native level shown by the facts line in Ait-managed child-agent rows and their compact-layout
sheets. Regular agent tabs show the composer controls without this separate facts line.
The composer trigger and dropdown checkmark both show the
requested selection/catalog default, including while Pi has yet to apply it or has clamped it.
Clearing a previous explicit selection requests `medium`
and reads back Pi's effective level. Model selection is applied before thinking selection.
Extension `thinking_level_changed` events update runtime information while idle, during prompt
admission, and during a turn. Startup and resume apply explicit host selections; later updates
apply only changed model/thinking fields. An unchanged host thinking selection does not overwrite
an extension's change on the next prompt, including after a model-only or mode-only update.
These controls do not rewrite Pi's global defaults or credentials.
Native thinking metadata is optional: unknown string levels are reported as received, and missing
or non-string state values leave the runtime level unset. Change events without a string level
are ignored. These variations do not prevent sessions or turns from running; explicit host
selections still require a supported control and a successful native response.

Pi-native subagents created by extensions are not integrated into Ait's subagent list or
sheet navigation. Installing a Pi subagent extension alone does not enable those views.
The facts line applies to child agents whose parent relationship is managed by Ait.

## Import an existing session

Open the existing session import flow and select a Pi conversation. Ait resolves the daemon host's
session location using Pi's normal precedence: `PI_CODING_AGENT_SESSION_DIR`, global/project
`settings.sessionDir`, then `sessions` under `PI_CODING_AGENT_DIR` or `~/.pi/agent`. It reads that
location without launching a Pi writer or modifying Pi's session file, settings, or credentials.
This is read-only only with respect to Pi-owned state: listing does not materialize images, but
inspection, import, and refresh decode embedded image data into private content-addressed files
under Ait's daemon data directory at `agents/provider-images`. Existing absolute-path and HTTP(S)
image references remain references. Invalid or unsupported embedded images, or failure to write
the private artifact, fail inspection.

User images display as thumbnails with full-image viewing on other devices through Ait's
authenticated file access. Live input, import, and refresh put image references in the existing
`imageSources` timeline field, with user text kept separately. Image-only messages are retained,
including native content with an empty text block alongside an image. Missing or non-string text
in a text block remains invalid. Assistant and tool-result images keep their Markdown rendering.
The sending client continues to prefer its original local attachments, which already displayed
before this projection change. The image payload sent to Pi is unchanged.
Existing Agents whose saved user images appear as Markdown text need explicit history refresh;
daemon restart preserves their saved timeline. See
[ADR-176](../decisions/providers/adr-176-pi-user-image-projection.md).

Cwd-filtered discovery honors project settings; an unfiltered cross-project search cannot locate
directories configured only inside unknown projects or relative to their working directories. A
cwd-filtered request resolves storage from the cwd spelling supplied by the caller, including
encoding that spelling for the default project directory. Canonicalization is used only to compare
the session header's directory identity. The request does not scan other project directories when
the resolved directory is absent. Listings include the title, working directory, and first/last
prompt previews.

Import follows the current branch, but it imports Pi's current model-context projection rather
than a verbatim copy of the transcript shown by Pi. The latest retained `context_edit` for an
editable message or visible custom message replaces or omits that content. When the branch has
been compacted, the latest compaction determines the projection: Ait places its summary first,
drops entries before `firstKeptEntryId`, excludes system messages from the retained pre-compaction
range, and then includes later entries. Consequently, an edited or compacted session can display
different content in Ait than in Pi. Within that projection Ait preserves text, reasoning, images,
tools, shell results, compaction markers, branch summaries, and visible extension messages.
Continuing uses the original native session UUID and file.

Stop any external Pi writer before importing. Incomplete turns, malformed or changing files,
unsupported message roles or file versions, and histories above 32 MiB, 100,000 records, or a
4,096-entry timeline projection are rejected. Version-three session files are supported. Custom
session files outside the configured session directory are not discovered. Automatic replay remains
disabled so ordinary restart retains Ait's persisted timeline. See
[ADR-174](../decisions/providers/adr-174-pi-session-import.md).

After Ait resumes a session, Pi's reported native session file becomes that Agent's pinned refresh
source. Refresh therefore continues to use the same verified UUID and canonical working directory
when the project was opened through a symlink or when the configured session directory later
changes. If that file disappears or becomes unusable, refresh falls back to identity lookup in the
currently configured project store; it does not scan unrelated project directories.

Ait passes the resolved host directory to its Pi writers with `--session-dir`. Environment values
on an individual Agent still configure that child process, but `HOME`, `USERPROFILE`,
`PI_CODING_AGENT_DIR`, and `PI_CODING_AGENT_SESSION_DIR` in `CreateRequest.env` do not relocate
session storage when host resolution succeeds. This keeps the ephemeral per-Agent environment out
of host-wide discovery. The default `~/.pi/agent` layout and any `~`-prefixed configured path
require `HOME` (or `USERPROFILE`) in the daemon's host environment for discovery and inspection.
If neither is available, those read paths remain unavailable unless the daemon itself has a
non-tilde `PI_CODING_AGENT_DIR` or `PI_CODING_AGENT_SESSION_DIR`. Creation and resume instead omit
`--session-dir` and let Pi resolve its own storage, including through the system account database;
in that fallback only, the child environment can affect Pi's native storage resolution.

## Session lifecycle and limits

Do not run another writer against the same native session while it is active in Ait.
A missing native file for a previously used session fails resume instead of creating a new
conversation under the old identity. Empty sessions can be resumed before Pi creates their
first persistent history file.

The adapter requires `agent_settled` to finish a run; `agent_end` can be followed by retry or
compaction work. Native inference after admission is not subject to a fixed host run timeout. Prompt
admission has a separate 120-second limit; slow extension commands that exceed it are
stopped with an uncertain outcome and are never automatically replayed. Ordinary controls
and abort settlement have 10-second limits. A `handled` response is not itself completion:
Ait checks native state and drains ordered events before completing or accepting another turn.
Handled commands running only manual compaction wait for its end and an idle state barrier,
within a 120-second settlement limit.

Inbound JSONL records are limited to 2 MiB including LF; outbound records to 64 MiB.
Oversized native frames produce a protocol diagnostic with source `frame_limit`. Display
queues normally hold at most 256 events, with reserved failure cleanup capacity. While
prompt admission prevents the manager from draining events, a separate budget allows
4096 events and 64 MiB of display payload. This combines cumulative text with currently
queued tool previews. Text deltas merge in chunks of at most 96 KiB during admission;
tool progress always keeps only the latest unpublished snapshot per item, and final
items replace their pending progress. Publishing a tool item releases its preview bytes.
Repeated updates and already-published tools therefore do not consume a lifetime tool
budget. Preview sizes are captured while rendering, without serializing whole tool items
for accounting. Strings count displayed UTF-8 bytes; structured previews count rendered
JSON bytes.

After admission, the text counter resets and the normal 8 MiB combined payload budget
applies once the admission backlog fits. The larger event and payload allowances remain
until that backlog drains, including during cancellation. Individual text items are
limited to 96 KiB, and each tool input/output preview to 32 KiB plus a truncation marker.
Full results remain in Pi's native history. Excessive distinct queued items, retained tool
previews, or cumulative text fail the host turn and close Pi, even if an extension has
completed its native work before acknowledging the command. Queue metadata, cached inputs
for at most 128 active tools, and reserved terminal cleanup are additional overhead.

Version checks are cached per configured adapter until daemon restart (transient errors are
not cached). Discovery still starts a temporary RPC process and creation starts a native
writer, so first use includes Node startup. An environment override has a separate version
cache. Discovery retains the host's 30-second total catalog budget.

## Project trust

Ait passes neither `--approve` nor `--no-approve` and never modifies Pi's trust store.
Pi 1.2 honors remembered/configured trust and native trust hooks. If project trust remains
undecided, RPC startup cannot ask interactively and ignores trust-gated project-local `.pi/`
settings, extensions, skills and packages. This differs from terminal Pi's interactive prompt;
it does not mean already-trusted projects are disabled. To establish trust, open Pi directly
in that project and make the trust decision there. Later extension UI dialogs are separate
and remain cancelled by this adapter.

## Validation

Focused deterministic tests:

```sh
cargo test -p provider local::pi --lib
cargo test -p provider composition::tests --lib
```

Optional installed-Pi test (isolated temporary configuration and a loopback fake model;
no real model service or user credentials):

```sh
cargo test -p provider local::pi::tests::installed --lib -- --ignored
```

Implementation decision: [ADR-156](../decisions/providers/adr-156-pi-native-rpc-provider.md).
Focused user-image projection results and validation limits:
[2026-10-21 report](../reports/providers/pi-user-image-projection-2026-10-21.md).
