# ADR-174: Pi native session import

- Status: Accepted.
- Date: 2026-10-18.
- Scope: `crates/provider`; extends [ADR-156](adr-156-pi-native-rpc-provider.md).

## Context

Pi's RPC adapter can resume Ait-created sessions, but users also need to import conversations
started in Pi. Native JSONL files contain a tree, not a flat transcript, and discovery must
not start a native writer or submit input.

## Decision

- Implement the existing session listing and inspection ports inside `provider::local::pi`.
  Resolve session storage from the daemon's host environment, in Pi's precedence order:
  `PI_CODING_AGENT_SESSION_DIR`, merged global/project `settings.sessionDir`, then `sessions`
  under `PI_CODING_AGENT_DIR` or `~/.pi/agent`. Relative and tilde paths use the target cwd
  and host home exactly as Pi does. The default layout and tilde expansion are unavailable when
  the daemon host provides neither `HOME` nor `USERPROFILE`; a non-tilde daemon-level
  `PI_CODING_AGENT_DIR` or `PI_CODING_AGENT_SESSION_DIR` remains usable. Pass the resolved project
  directory to Ait-owned Pi writers through `--session-dir`, so Pi-storage inspection and writing
  share one location. If host resolution is unavailable, omit that argument for writers and let Pi
  resolve its native storage, including through the system account database; creation and resume
  therefore remain available even though discovery and inspection do not.
  Per-Agent `CreateRequest.env` remains ephemeral child configuration and cannot relocate the
  host-owned session store through `HOME`, `USERPROFILE`, or either Pi directory variable when host
  resolution succeeds. In the missing-host-home writer fallback, Pi's child environment necessarily
  participates in its native storage resolution.
  Run filesystem inspection on blocking workers. Do not change the service or wire contracts,
  native history, credentials, or other provider-owned configuration.
- Discover by UUID, working directory, title, and first/last user preview. Filter directories
  before applying the requested result limit to valid, distinct session descriptors; malformed
  files and duplicate identities do not consume that result limit, but do consume listing work
  budgets. Bound enumeration to 16,384 candidate files and full reads to eight times the requested
  result limit, 64 MiB total, and a two-second elapsed-time budget checked between filesystem
  operations. Each file remains limited to 32 MiB and 100,000 records, and inspection rejects a
  projection above 4,096 timeline entries. Cache default-layout cwd
  recovery once per project directory. Recent-session listing and native inspection share the
  manager's history semaphore. Skip invalid listing candidates. Reject
  symlink candidates, incomplete JSONL, changed reads, unsupported versions, broken ancestry, and
  identity/directory mismatches during inspection. Reject duplicate IDs within the selected project
  while inspecting a new import. Shared session filters remain canonical for existing providers. Pi
  additionally receives the validated caller spelling for storage lookup: normalize its lexical `.` and
  `..` components and trailing separators without resolving symlinks before encoding the default
  project folder. Canonicalization is used to compare the session header's directory identity and for
  imported Agent placement and the resume metadata `cwd`. Preserve the validated lookup spelling
  separately as provider-owned `nativeCwd` through inspection and live-session persistence. Import
  and unknown-handle resume pass the caller spelling to inspection before canonical placement.
  Inspection validates `nativeCwd` against the requested canonical directory before using it.
  A missing folder does not trigger a cross-project
  fallback. Sessions launched through a different symlink spelling remain available to unfiltered
  discovery.
- Follow the final entry's parent chain. Project user text/images, assistant text/reasoning,
  tools, manual shell executions, compaction markers and summaries, branch summaries, and
  visible custom messages. Apply the latest retained `context_edit` to editable message and
  custom-message content, including omission. When the branch contains compaction, match Pi's
  context projection: put the latest summary first, keep the pre-compaction range beginning at
  `firstKeptEntryId` without system messages, and then keep later entries. Reuse Pi's live tool
  rendering. Reject malformed context edits and unsupported message roles instead of silently
  importing incomplete display history. Unknown non-message metadata is ignored. This is a
  model-context projection, not a verbatim copy of Pi's displayed transcript: edits can replace
  or remove displayed rows, and compaction can remove older rows from Ait's imported timeline.
- Keep Pi-owned state read-only during listing and inspection, but do not describe inspection as
  globally side-effect free. Listing reads bounded Pi-owned metadata and history without
  materializing images. Inspection, import, and refresh use the
  shared provider image store to materialize embedded image bytes as private content-addressed
  files under `agents/provider-images`; absolute-path and HTTP(S) sources remain references.
  Reject invalid or unsupported embedded images and safe-storage failures rather than embedding
  base64 in the timeline or silently omitting the image.
  [ADR-176](adr-176-pi-user-image-projection.md) specifies user-image projection through the
  existing `imageSources` field, shared with live Pi input; assistant and tool-result images
  retain their Markdown projection.
- Treat a final unanswered user message, unresolved tool call, or tool result awaiting an
  assistant continuation as incomplete. The existing import service rejects incomplete
  histories. File inspection cannot prove another Pi process is idle; users must stop
  external writers before importing or continuing in Ait.
- Store the verified native filename in provider resume metadata alongside cwd and `hasInput`.
  Before the first resume, refresh reopens that regular file only after verifying that it remains
  immediately beneath the configured session store. Interactive resume accepts this metadata when
  an imported handle has no `native_handle`, then persists Pi's reported session file as the
  provider-native handle. That established handle remains pinned across cwd symlink spelling and
  later session-store configuration changes; refresh still requires an absolute regular `.jsonl`
  file with a non-symlink parent and verifies its native UUID and canonical cwd. A missing or
  unusable pinned file falls back to project-scoped identity lookup in the current store, using
  `nativeCwd` when present. Interactive resume uses the same inspector to resolve missing filenames
  before opening a writer, and unknown-handle resume retains the inspector's verified metadata.
  Legacy handles without `nativeCwd` use the supplied cwd spelling; a handle that has already lost
  both its filename and original spelling must be resumed with that spelling or re-imported.
- Keep automatic history replay disabled: live Pi events and saved entries use different
  identities. Explicit import/refresh reconciles native history; ordinary restart preserves
  Ait's durable timeline. Thinking controls and rewind remain unsupported.

## Consequences

Pi appears in the existing recent-session import flow and imported conversations can continue
in the original native session. Oversized, incomplete, or unsupported histories fail explicitly.
Recent-session discovery can return fewer than the requested result limit when its candidate,
byte, or elapsed-time budget is exhausted, and concurrent discovery cannot bypass the shared
history admission limit.
Sessions stored outside Pi's host-configured session directory are not discovered. A cwd-filtered
listing honors project-local and cwd-relative session directories; an unfiltered cross-project
listing cannot infer custom directories declared only inside arbitrary projects or relative to
their working directories. Inspection does not launch a Pi writer or modify Pi-owned state, but
may create Ait-owned image artifacts. A missing host home prevents discovery's default layout and
tilde expansion but no longer prevents Pi writer creation or resume. Because import applies retained
context edits and the latest compaction boundary, Ait's timeline can intentionally differ from the
transcript Pi displays.
An established Agent can continue refreshing its pinned native file after session-store settings
change, while discovery and fallback lookup use only the new configured store.
