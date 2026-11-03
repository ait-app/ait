# OpenCode native command lifecycle proposal

[command-lifecycle.patch](command-lifecycle.patch) targets official OpenCode 2.1.3,
`bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1`. It is a reviewable upstream
proposal, not a released API, bundled fork, installation hook or build step.
The upstream MIT notice is preserved in [LICENSE](LICENSE).

In this release, ACP calls the native command handler and returns `end_turn`
immediately. The handler may only have enqueued its model input. It also omits
ACP's generated input ID, so the ACP event translator cannot correlate that
input. Under load, replay can return before even the input appears, and asks or
output can outlive the ACP request.

The proposal keeps the existing command HTTP endpoint unchanged. An additive
experimental `session.submitCommand` endpoint accepts a native input ID and
returns the actual `SessionInbox.User` receipt, when the handler admits
foreground model work. Existing local handlers keep their ignored return values;
only schema-valid native receipts (typed or serialized) enter ACP settlement. The
built-in configuration command handler forwards the ID and returns its original
prompt admission receipt, retaining native template expansion, models,
permissions and agents. ACP joins its existing event consumer only when that
receipt matches its input; local handlers finish immediately. ACP advertises
`agentCapabilities._meta["opencode/command-lifecycle"] = 1`.

Handlers admitting foreground model work should preserve `invocation.prompt.id`
and return the admission receipt. Background/subagent command parity and
arbitrary third-party handlers need separate lifecycle verification; this patch
does not claim to complete them. ACP argument parsing also retains quoted
whitespace and multiline arguments rather than splitting and rejoining them.

Ait guards the verified unmodified 2.1.3 profile before submitting affected slash
commands. `/compact` and normal prompts keep their existing native paths. The
guard is removed for the negotiated receipt capability, not by timing guesses,
extra prompts, screen parsing, transcript polling or another native writer.

Validation separates the official binary (`AIT_TEST_OPENCODE_BIN`) from the
isolated source runtime (`AIT_TEST_OPENCODE_COMMAND_BIN`). The latter runs the
native CLI, ACP, core command handler and loopback mock model; the Ait test still
requires the final authoritative answer. The source suite covers delayed
completion, local actions, approvals, cancellation, invalid receipts and exact
arguments, alongside existing prompt/turn regressions. Generated clients are
included; regenerate them with `bun run --cwd packages/client generate`.

See [the adaptation report](../../docs/reports/providers/dsh-opencode-capability-adaptation-2026-10-30.md)
for current limits and validation results.

To reproduce the upstream checks, check out the pinned commit in a separate
clone, apply the patch with `git apply --check` followed by `git apply`, and
install its declared Bun dependencies. Run `bun run generate` in
`packages/client`, `bun run typecheck` in `packages/core` and `packages/cli`,
and these focused tests from their owning package directories:

```sh
# packages/cli
bun test --timeout 30000 test/acp/command-lifecycle.test.ts test/acp/prompt.test.ts test/acp/turn.test.ts
# packages/core
bun script/test.ts test/command.test.ts test/config/command.test.ts test/config/command-subagent.test.ts
```

A source CLI reports a local version unless its build defines the release
version. The Ait installed test uses a separately built/wrapped pinned CLI,
never a renamed unpatched binary. It preserves native standalone-server setup
and uses the CLI's normal Solid preload. Build the native CLI using its own
build instructions before setting `AIT_TEST_OPENCODE_COMMAND_BIN`.
