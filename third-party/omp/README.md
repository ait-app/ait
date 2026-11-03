# OMP native completion proposal

[native-completion.patch](native-completion.patch) is a reviewable upstream proposal for OMP 18.14.0,
tag `v18.14.0` (`31147f9d1a76608549821c3759f8487b4d66c72d`). It is
not applied by Ait installation or builds. The upstream code is MIT licensed;
its notice is preserved in [LICENSE](LICENSE).

The patch extracts existing TUI command builders and their live completion
context. TUI, ACP and RPC share builtin argument completion, extension/custom
callbacks, file commands and template precedence. Ait does not copy the completer.

The proposed ACP extension advertises `_meta.ompCommandCompletion.version = 1`
in initialize and accepts `_omp/commands/complete` with `{sessionId,text,cursor}`.
RPC advertises `ready.capabilities.commandCompletion.version = 1` and accepts
`complete` with `{text,cursor}`. Both return `{supported:true,items}`. Offsets use
UTF-16 code units; candidates include exact edits and resulting carets.

Lookup does not submit prompts or invoke `onApplied` callbacks. Candidates that
leave both text and caret unchanged are omitted so Enter can submit a completed
command. RPC lookup runs
outside the serial command queue, so an unanswered form cannot block it. Limits:
64 KiB input/resulting text, 256 candidates, 896 KiB candidate data, 750 ms lookup.
Newer lookup aborts the previous one. Generated schemas and Python/Rust/Go wire
definitions are included.

Only remotely executable builtins are offered. Arbitrary TUI factories, editor
actions with local callbacks and TUI-only dashboards still need semantic remote
operations; this patch does not claim to implement them.

Isolated validation uses the official npm source package and embedded Bun from
`/usr/bin/omp`, with test-only models and plugins. Rust tests distinguish the
unmodified installed binary (`AIT_TEST_OMP_BIN`) from the patched source runtime
(`AIT_TEST_OMP_COMPLETION_BIN`). The latter verifies native ACP, three levels of
plugin completion, and lookup while an extension form remains unanswered.

For upstream review, apply the patch to the recorded release source, run the
focused `test/slash-commands/session-completion.test.ts`, regenerate RPC artifacts
with `bun run gen:rpc`, and run package checks. See
[ADR-212](../../docs/decisions/providers/adr-212-native-editor-completion.md)
for the Ait integration boundary.
