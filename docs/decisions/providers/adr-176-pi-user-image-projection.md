# ADR-176: Pi user images in the shared timeline projection

- Status: Accepted (implemented).
- Date: 2026-10-21.
- Scope: `crates/provider`; extends the user-image projection from
  [ADR-157](adr-157-codex-user-image-projection.md) to Pi's live input and
  [ADR-174](adr-174-pi-session-import.md) native history import and explicit refresh.

## Context

Pi previously projected user images as inline image Markdown in the user message's
`text`. The sending client already displayed its local attachments correctly. Imported
history and clients without those local attachments instead received image Markdown,
while the shared user-message renderer consumes image references from `imageSources`
for previews. ADR-157 introduced that field for Codex; Pi needs an explicit
provider-to-timeline contract using the same client behavior. This changes the display
projection, not the image payload sent to Pi.

## Decision

- Pi owns the conversion of its live input and native user messages to existing
  `user_message` timeline items. Both paths share the same row construction:
  `text` contains user text, and optional `imageSources: string[]` contains ordered
  image references. Native image blocks no longer generate Markdown in user text.
  This reuses the existing protocol field without changing the wire schema or adding
  a client-side Pi parser. Assistant and tool-result image rendering retains its
  existing Markdown projection.
- Embedded image bytes use the shared private, content-addressed provider image
  store under `agents/provider-images`. Timeline items contain host paths rather
  than base64 or data URLs; absolute-path and HTTP(S) native image sources remain
  references. Invalid or unsupported embedded image data and safe-storage failures
  fail projection instead of silently dropping images. Listing does not materialize
  images, and Pi-owned files remain read-only during inspection.
- Keep a user row when text is empty and images are present. Accept empty strings
  in native text blocks, including an empty text block alongside an image; missing
  or non-string block text remains invalid. Import and refresh project the latest
  retained context edit, including replacement or omission of user images, within
  ADR-174's selected branch and compaction boundary.
- Split long user text using Pi's existing 96 KiB chunk limit and stable row keys.
  Put `imageSources` and any live `clientMessageId` only on the first row; text
  continuation rows keep independent identities and do not repeat previews.
- Shared clients load host image paths through existing authenticated file RPC and
  the image-preview lifecycle, including thumbnail and full-image viewing on other
  devices. The sending client continues to prefer its original local attachments.
- Do not rewrite Pi's native input or history, or automatically migrate persisted
  Ait timelines. Explicit import/refresh rebuilds the display projection using the
  existing timeline generations, cursors, and timestamp retention. Ordinary restart
  preserves the saved timeline, and automatic Pi history replay remains disabled.

## Consequences

Pi user images use the same cross-device display contract as Codex, including
image-only messages. Clients must support `imageSources` to display these previews.
Existing Pi Agents with the older Markdown projection need explicit history refresh
to update their saved display; restart alone does not migrate them. This adds no
crate dependency, database schema, file-transfer interface, or native-history writer.

## Test coverage

Line coverage was not measured because it was not requested. Relevant regressions
cover live image-only input, text chunking with one preview set, imported user images,
context edits, empty text blocks with images, repeated inspection, and unchanged native
history files during inspection. The [validation report](../../reports/providers/pi-user-image-projection-2026-10-21.md)
records the tested source snapshot, exact commands, and execution results separately from
coverage. If measurement is requested, use focused provider tests with `cargo llvm-cov`
and record the revision, command, and artifact.
