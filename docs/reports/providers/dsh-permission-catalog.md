# DSH permission catalog compatibility

DSH `0.2.0-rc.2` projects only the current permission selection into session history. Its selectable presets now come from `permissionPresets/catalog`. Ait previously required an embedded options array, failing during provider discovery, creation and resume. The adapter now retrieves the native catalog when the array is absent; older Hosts keep using their embedded options. Malformed projections fail without inventing defaults or widening permissions.

## Test coverage

Measured source: Git tree `dcaff245b2e3222d16873e3fc7cc2723d7ed2b34`, based on `f83d9579`. Linux x86_64, default features, no source exclusions; 12 installed-provider tests ignored. No comparable baseline. macOS/Windows were not exercised.

```sh
CARGO_TARGET_DIR=/home/lonnet/.cache/ait-pr-targets/dsh-permission \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

| Scope | Covered / total lines | Line coverage |
|---|---:|---:|
| Workspace | 54748 / 57971 | 94.44% |
| Provider | 26258 / 27964 | 93.90% |
| Native selection configuration | 174 / 184 | 94.57% |
| Native session | 353 / 401 | 88.03% |

This checked-in HTML-index summary is the reviewable coverage artifact. Full HTML remains local under the target directory, not uploaded. Real installation tests are separate and do not contribute to the percentage. Remaining gaps include native process/platform failures and dynamic catalog changes after session attachment.

## Test execution

Workspace: 1925 passed, 12 ignored; build, clippy (`-D warnings`) and rustfmt passed. Nine focused session tests passed, including separate native catalogs with custom choices, older embedded options without a new endpoint, and malformed projections rejected before permission commands.

The installed Linux desktop package `0.2.0-rc.2` was exercised through its bundled Node/CLI in a temporary launcher. An isolated native test passed discovery, session creation, permission switching, native resume and legacy ACP adoption without an external model request.

Two explicit installed-history tests passed on a user-selected real session: read-only import, then interactive adoption after the user released the desktop writer. The native session ID, model, permission and complete imported message history were preserved. No prompt was submitted. The first adoption attempt was correctly rejected by DSH with `session/writer-held` while its desktop owned the session; no lock was removed or process terminated.

Reproduction commands (choose an idle, released session yourself; IDs and user data are not checked in):

```sh
AIT_TEST_DSH_BIN=/path/to/dsh cargo test -p provider installed_host_discovers_switches_permissions_and_adopts_legacy_sessions -- --ignored
AIT_TEST_DSH_BIN=/path/to/dsh AIT_TEST_DSH_SESSION_ID=SELECTED_SESSION cargo test -p provider installed_existing_session -- --ignored
```

This validates the adapter's import and adoption path, not a GUI click-through or a new model turn. Ait must still be rebuilt with the fix, and its daemon must be able to launch the DSH CLI. A desktop GUI launcher alone is not a CLI.
