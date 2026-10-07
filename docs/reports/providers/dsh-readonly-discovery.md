# DSH read-only model discovery

The provider menu previously created and initialized a session before returning model choices. A failed default session therefore hid an otherwise readable model catalog and left probe history. Discovery now reads only `session/modelCatalog`, retaining the explicit ACP compatibility path.

## Test coverage

Measured source: Git tree `f2294b21abc2ce7480fdc054a3c7dab708e553cd`, based on `f83d9579`. Linux x86_64, default features, no source exclusions; 10 installed-provider tests ignored. No comparable baseline. macOS and Windows were not exercised.

```sh
CARGO_TARGET_DIR=/home/lonnet/.cache/ait-pr-targets/dsh-discovery \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

| Scope | Covered / total lines | Line coverage |
|---|---:|---:|
| Workspace | 54762 / 57984 | 94.44% |
| Provider | 26272 / 27977 | 93.91% |
| Native discovery | 25 / 25 | 100.00% |

This checked-in HTML-index summary is the reviewable coverage artifact; full HTML remains local under the target directory, not uploaded. Installed CLI behavior is measured separately and not included in coverage. Native startup and platform-specific failures need broader installation coverage.

## Test execution

Workspace: 1923 passed, 10 ignored. Build, clippy (`-D warnings`) and rustfmt passed. Regression coverage confirms discovery succeeds without session permission projections and never creates a session, selects a model or submits commands/input.

Installed CLI `0.1.5-rc.2`: discovery, permission switching and legacy adoption test passed using temporary native storage, without a model request. The locally installed desktop distribution `0.2.0-rc.2` also successfully returned model choices through its bundled CLI, but the subsequent create-session assertion failed: its permission projection no longer embeds options. That separate protocol compatibility issue is addressed independently; this change does not claim to fix session creation on its own.

The desktop package exposes a GUI executable rather than `dsh` on PATH on the tested machine. A temporary test launcher used the bundled Node and CLI entry. No user launchers, credentials or running Ait/DSH application were modified. This does not establish the cause of the inaccessible macOS screenshot.
