# Native session prompt previews

OpenCode imports now read the first and latest user text from each session history using its own working directory. DSH reads cached turn outlines and falls back to bounded history pages when previews are absent. Failed history reads retain the importable session. Previews normalize whitespace and stop at 300 Unicode characters; tool, assistant, synthetic and ignored content are excluded.

## Test coverage

Measured source: Git tree `f1640bfc475416ad5fdcd8bdf922909b1f96ec0e`, based on `f83d9579`. Linux x86_64, workspace default features, no source exclusions; 10 installed-provider tests ignored. No comparable baseline. macOS/Windows and the desktop UI were not exercised.

```sh
CARGO_TARGET_DIR=/home/lonnet/.cache/ait-pr-targets/preview \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

| Scope | Covered / total lines | Line coverage |
|---|---:|---:|
| Workspace | 54942 / 58177 | 94.44% |
| Provider | 26454 / 28170 | 93.91% |
| DSH history preview fallback | 110 / 119 | 92.44% |
| Shared preview formatting | 18 / 20 | 90.00% |

This checked-in summary is the reviewable coverage artifact. Full HTML remains local under the specified target directory and is not uploaded. Remaining uncovered paths include timeout/transport failures; real provider versions and macOS need follow-up validation.

## Test execution

Workspace: 1927 passed, 10 ignored. Workspace build, clippy (`-D warnings`), and rustfmt passed. Focused tests exercise first/latest user text, Unicode truncation, synthetic content filtering, cached DSH outlines, history fallback, and retaining entries after a history read failure.

OpenCode hydrates up to four histories concurrently, with two seconds per history and ten seconds for the scan. DSH fallback has the same time bounds plus 100 pages and 8 MiB per session. Sessions beyond these bounds remain importable without a preview.
