# Pi native RPC provider validation

Date: 2026-10-18 (Australia/Perth). Validated the uncommitted Pi implementation on
base revision `a9551f16` (Ait 0.1.7), on Linux x86_64 with default Rust features and
installed Pi 1.2.0. These results describe that working tree, not the base commit alone.

The adapter uses the existing provider ports for discovery, native sessions, streaming,
cancellation, compaction and diagnostics. The implementation boundary is recorded in
[ADR-156](../../decisions/providers/adr-156-pi-native-rpc-provider.md); setup and supported
behavior are documented in the [Pi operations guide](../../operations/pi.md).

## Validation

91 focused test executions passed in the rerun on Linux x86_64 with Pi 1.2.0:

- Pi adapter and transport: 57
- Installed Pi loopback integration: 1
- Provider composition and registration: 4
- Provider diagnostics: 14
- Provider catalog diagnostics: 2
- Daemon status, diagnostics, restart, and shutdown: 3
- MCP native-agent provider contract: 1
- Shared protocol tool projection schema: 9

The installed-Pi test used isolated temporary configuration and a loopback fake model.
It did not access a real model service or user credentials. It verifies discovery,
streaming, native session resume, rich input and undecided project trust behavior.
The regular Pi run skipped this opt-in test; it was then explicitly executed separately.
Tests requiring loopback listeners or uncached dependencies were rerun with the
necessary environment permissions. No source changes were needed.

Commands and passing test counts:

| Command                                                                                                  | Passed |
| -------------------------------------------------------------------------------------------------------- | -----: |
| `cargo test -p provider local::pi --lib`                                                                 |     57 |
| `cargo test -p provider local::pi::tests::installed --lib -- --ignored`                                  |      1 |
| `cargo test -p provider composition::tests --lib`                                                        |      4 |
| `cargo test -p provider diagnostics::tests --lib`                                                        |     14 |
| `cargo test -p provider service::provider_catalog::tests::diagnostics --lib`                             |      2 |
| `cargo test -p daemon --test process daemon::`                                                           |      3 |
| `cargo test -p daemon --test process mcp::mcp_native_agent_calls_preserve_parentage_and_input_admission` |      1 |
| `npm run test --workspace=@ait/protocol -- src/messages.tool-call-schema.test.ts`                        |      9 |

The diagnostics filter includes shared local and Antigravity diagnostics tests as well
as the top-level provider diagnostics tests; its count is not Pi-only coverage.

Rustfmt, tracked diff checks, strict all-target Clippy for `provider` and `daemon`,
changed-file Oxfmt and Oxlint checks, and documentation checks passed:

```sh
cargo fmt --all --check
cargo clippy --locked -p provider -p daemon --all-targets -- -D warnings
git diff --check
node_modules/.bin/oxfmt --check apps/mobile/e2e/support/helpers/isolated-host-daemon.ts packages/protocol/src/messages.tool-call-schema.test.ts crates/provider/tests/fixtures/pi_tool_cards.json docs/README.md docs/architecture/README.md docs/decisions/README.md docs/decisions/providers/adr-144-provider-error-reporting.md docs/decisions/providers/adr-150-provider-compaction-lifecycle.md docs/decisions/providers/adr-156-pi-native-rpc-provider.md docs/operations/daemon.md docs/operations/pi.md
node_modules/.bin/oxlint apps/mobile/e2e/support/helpers/isolated-host-daemon.ts packages/protocol/src/messages.tool-call-schema.test.ts
npm run check:docs
```

The documentation check verified links in 383 Markdown documents. The documentation
checker test also passed and is separate from the 91 focused test executions above.

The full Rust workspace suite also passed with `cargo test --workspace`, including all
workspace unit/integration tests and doctests. `cargo nextest` was unavailable in this
environment, so Cargo's workspace test runner was used as the documented equivalent.

## Limits

Native history import/refresh, rewind, live steering, host-provided MCP/tool policies,
custom system prompts, thinking-level selection, structured output and auxiliary summaries
are not exposed. Pi owns authentication, native tools and project trust. Interactive
extension dialogs are cancelled, and concurrent writers to one native session are unsupported.

## Test coverage

**Not measured**: coverage measurement was not requested. Test execution counts are not
coverage percentages; no coverage artifact or comparable baseline was produced. If later
requested, use `cargo llvm-cov` for the Pi adapter and directly related tests and publish
the focused report with its exact revision, commands, features and exclusions.
