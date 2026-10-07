# Atomic PR integration validation

A temporary local integration branch combined the individual changes with main `76880f66`. No combined PR was created. Source conflicts were limited to combining two independently added SDK tests; documentation conflicts retained both entries and the auxiliary channel sections. ADRs use 097 for auxiliary models and 098 for chunk transport, leaving upstream 095/096 intact.

## Test coverage

Measured source tree: `463c8caa893a405d1040867d0659e78ef475e522`. Workspace, Linux x86_64, default features, no source exclusions, 14 installed-provider tests ignored. No comparable combined baseline; macOS/Windows were unavailable.

```sh
CARGO_TARGET_DIR=/home/lonnet/.cache/ait-pr-targets/integration \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

| Scope | Covered / total lines | Line coverage |
|---|---:|---:|
| Workspace | 55314 / 58815 | 94.05% |
| Provider | 26696 / 28682 | 93.08% |
| API | 2093 / 2251 | 92.98% |
| Filesystem | 11562 / 12187 | 94.87% |

This checked-in HTML-index summary is the shared reviewable artifact. Full HTML remains local and is not uploaded. Native installed-process paths are not covered by the default suite; the separate installed checks below do not contribute to these percentages. A later lint-only change replaced intermediate strings in the ignored Messages API fixture with direct writes; both installed-version checks and clippy were rerun, while the table remains the measurement of the tree named above. Production sources are unchanged by that fixture edit.

## Test execution

- Rust workspace: 1947 passed, 14 ignored (`cargo test --workspace`, isolated target and config, four test threads).
- Workspace build, rustfmt check and clippy `--workspace --all-targets -- -D warnings` passed. Clippy's initial fixture allocation finding was corrected and rechecked.
- Combined mobile tests: 33 passed across directory drops, ordinary attachments, model selection, file links and preview lifecycle. Mobile TypeScript passed.
- Combined SDK: four upload acknowledgement/model-notice tests passed, 139 unrelated tests skipped by filter.
- DSH auxiliary generation: installed CLI 0.1.5-rc.2 and desktop-bundled CLI 0.2.0-rc.2 both passed using loopback endpoints and temporary native storage. Requests contained no tools and left no persisted auxiliary session. The fixture now supports both Chat Completions and Messages API streams.
- Real DSH history import and adoption were verified separately in [the permission compatibility report](https://github.com/KirisameLonnet/ait/blob/fix/dsh-permission-catalog/docs/reports/providers/dsh-permission-catalog.md). No user prompt was sent.
- Documentation links passed. No GUI drag-and-drop, macOS/Windows execution, or public relay throughput claim is made.

## Combined inputs

These source heads plus the Messages API test fixture form the measured integration tree. The integration branch is local; each production change remains in its own PR.

- `fix/desktop-broken-transport`: `5a8d18417e5b5d19c954b7ffc69645a48ab81c06`
- `fix/model-selection-feedback`: `6407b4981c85fad26a8f9f0d47bc59814e5113f2`
- `fix/system-directory-opener`: `bf861d163afae034b94301515ccf0902f7ff0966`
- `refactor/provider-metadata-models`: `052d005f92c5c5b0e9b4d1e67c3cd7d87e9c74a3`
- `fix/workspace-creation-null-agent`: `09d8e987d049657e0acfb92e861b75f20407f0de`
- `feat/dsh-auxiliary-generation`: `fe7513b6899387827e89cf91c4ac2aa7242417d5`
- `feat/opencode-auxiliary-generation`: `d2224800f22d4a22efa142fee714a838961b6747`
- `feat/chunked-client-messages`: `0b3b365e185040995d72b3432d9214cdcf420cdc`
- `fix/native-session-preview`: `dae905a024a044f1a80253f1af93773cbe635f5a`
- `fix/dsh-readonly-model-discovery`: `f8b22f6f01782d16bb2731df59f5d9eb2648a540`
- `fix/composer-directory-drop`: `ce24db6230a2e275af0ee5651cd25da9b93995d6`
- `fix/dsh-permission-catalog`: `214ecf20ecfff120a0b51db849fa705b954a22a7`
