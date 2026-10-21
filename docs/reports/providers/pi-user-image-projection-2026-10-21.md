# Pi user-image projection validation

Date: 2026-10-21. Validated the uncommitted provider changes based on
`b2e9b093dd7f55d588b381dff8662458d15696a6`, with Cargo's default features.
These results belong to the provider image-projection PR. They do not reuse the
workspace results recorded for the earlier Pi import implementation.

The display contract is recorded in [ADR-176](../../decisions/providers/adr-176-pi-user-image-projection.md).
User-facing behavior is documented in the [Pi operations guide](../../operations/pi.md).

## Environment and source snapshot

- Linux x86_64.
- Rust: `rustc 1.98.1 (48a229cea 2026-09-01)`.
- Cargo: `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
- Installed Pi: `/usr/bin/pi`, version `1.2.0` (verified with `pi --version`).
- Default features; no feature overrides or test-parallelism reductions.

SHA-256 identifies the tested working-tree files independently of the eventual PR commits:

```text
ccd386993c4428dbc4d8a072ac1f49d56d27347265bed6b9627c3cd5537cef26  crates/provider/src/local/pi/history.rs
6d3d3c6594a14be3c23b0591d52a90810d12c1a4978e729b82051cb1bff1a496  crates/provider/src/local/pi/history/tests.rs
07e4a48d2aa41d2639c0aa354ad5b7322092c37c6afef9541272a236b8f176f3  crates/provider/src/local/pi/streaming.rs
40444a8723ff7ee770253eed4eda534b31b9fd696eb1c7b1388e2db7ddd273dd  crates/provider/src/local/pi/tests/text_storage.rs
a053ba14b853e51bf2015fe239c342054b510b17834793787260fbe26d10221a  crates/provider/src/local/pi/tests/installed.rs
```

## Focused provider tests

```sh
cargo test --locked -p provider --lib local::pi
```

Passed with exit status 0: **125 passed, 0 failed, 1 ignored, 982 filtered out**.
The regular filter skips the optional installed-Pi case, which was subsequently executed
explicitly as recorded below. The command compiles the provider unit-test target and runs
only the Pi filter; it is not a full provider-crate or Rust workspace test run.

Both newly added history regressions passed:

- `empty_user_text_blocks_with_images_allow_inspection_and_refresh` checks original
  and context-edited content, retained image bytes, stable repeated inspection, and
  unchanged native history.
- `image_only_user_context_edit_projects_a_preview_without_markdown_text` checks
  image-only replacement content and stable inspection.

All three updated regressions passed:

- `projects_images_compaction_extension_messages_and_manual_shell_commands` checks
  separate user text and image sources alongside the existing history projection.
- `submitted_rich_input_keeps_identity_and_fits_storage_entries` checks text chunking,
  first-row image references and client identity, stored image bytes, and timeline storage.
- `submitted_image_only_session_is_materialized_for_timeline_display` checks that a live
  image-only input produces a user row with an empty text field and a readable image source.

The remaining Pi tests also passed, including discovery, context edits, compaction,
resume, restart retention, stream admission, cancellation, and transport cleanup.

## Installed Pi integration

```sh
cargo test -p provider local::pi::tests::installed --lib -- --ignored
```

Executed against installed Pi **1.2.0**, with permission to bind the test's isolated
loopback model server. Passed with exit status 0: **1 passed, 0 failed, 0 ignored,
1,107 filtered out**, in 1.88 seconds.

`installed_pi_streams_and_resumes_with_loopback_model` verifies:

- Real Pi model discovery without creating persistent sessions during discovery.
- Native session creation, close/resume, history listing/inspection, import metadata,
  and continuation using the same native session identity.
- Streaming completion and native shell, write, edit, and failed-tool cards.
- A resumed rich-input turn forwarding text attachment content and a valid PNG image
  into the loopback model request.
- Undecided project trust preventing a project-local extension from executing.

The test uses temporary configuration, native session files, and a fake model on
loopback. It does not use user provider credentials or an external model service.
It exercises the installed runtime and native adapter paths; it does not assert browser
thumbnail rendering or full-image viewing.

Together with the earlier regular Pi run, the focused validation records **126 passing
test executions, 0 failures**. The installed case was skipped in the regular run and
passed in this separate invocation; it is counted once in that total. The provider source
hashes above were unchanged when the installed test ran. The regular suite and Clippy
were not rerun for this report-only update.

## Build, lint, formatting, and documentation

```sh
cargo clippy --locked -p provider --all-targets -- -D warnings
cargo fmt --all --check
node_modules/.bin/oxfmt --check docs/README.md docs/decisions/README.md docs/decisions/providers/adr-157-codex-user-image-projection.md docs/decisions/providers/adr-174-pi-session-import.md docs/decisions/providers/adr-176-pi-user-image-projection.md docs/operations/pi.md docs/reports/README.md docs/reports/providers/pi-user-image-projection-2026-10-21.md
npm run check:docs
git diff --check
git diff --cached --check
```

Provider Clippy completed with exit status 0 and no warnings. The focused test and
Clippy commands compiled their selected provider targets successfully. Rust formatting,
changed-document formatting, documentation checker tests, local-link validation, and
staged/unstaged whitespace checks passed. Documentation checks were repeated after adding
the validation reports and verified local links in 419 Markdown documents.

## Behavior and limits

The sending client already displayed its local attachments. This change supplies
structured image references when local attachments are unavailable, including imported
history and clients on other devices. It does not change the image payload sent to Pi.
Assistant and tool-result images retain Markdown rendering. Existing saved timelines
need explicit refresh; ordinary restart does not migrate their display projection.

Installed-runtime validation is limited to Pi 1.2.0 and the isolated loopback model.
This report does not claim a real model service, physical-device verification, or a full
Rust workspace run. The follow-up browser acceptance suite has separate execution results
and is not included in the 126-test focused total.

## Test coverage

Coverage was not measured because it was not requested. The counts above describe test
executions, not line or branch coverage. If requested, measure the focused provider scope
with `cargo llvm-cov` and record the tested revision, enabled features, exclusions,
covered/total lines, comparable baseline if available, and a shared report artifact.
