# Rust code smell 清理：PR 验证

源码提交：`46980159e8ec289002adbc4d7224840159d01d3b`。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。Cargo 命令使用默认 features 和离线模式。`SHERPA_ONNX_LIB_DIR` 指向本机缓存的 sherpa-onnx v1.13.8 macOS arm64 静态库。

## 变更范围

- 将公开 Workspace 字段统一为单层 `Option<T>`，在私有 checkout 输入解析层保留缺失值与显式 `null` 的区别；边界决定见 [ADR-082](../../decisions/workspace/adr-082-canonical-nullable-workspace-fields.md)。
- 清理无用的 lint `allow`、不可达分支和多余克隆，简化 Agent thinking filter、OpenCode 投影与 terminal 服务代码。

## 构建、测试与静态检查

以下 Cargo 命令均设置 `SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`。

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace --offline` | 通过；0 failed，3 ignored |
| `cargo build --workspace --offline` | 通过 |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | 通过 |
| `cargo fmt --all --check`、`git diff --check` | 通过 |
| `npm run check:docs` | 通过，194 个 Markdown 文档的本地链接有效 |

3 项 ignored 测试需要本机 Provider CLI 或认证。完整测试和覆盖率运行需要允许本机监听的环境。

## Test coverage

`SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib cargo llvm-cov --workspace --html --offline` 插桩运行通过，3 项测试 ignored。`cargo llvm-cov report --json --summary-only --output-path /private/tmp/wretched-flamingo-cov-raw.json` 提取行统计。

| 范围 | 已覆盖 / 总行数 | 行覆盖率 |
| --- | ---: | ---: |
| Rust workspace | 49,035 / 51,889 | **94.50%** |
| filesystem | 11,146 / 11,737 | 94.96% |
| metadata | 7,526 / 7,950 | 94.67% |
| provider | 21,301 / 22,666 | 93.98% |
| terminal | 1,486 / 1,543 | 96.31% |

[可审查的覆盖率产物](rust-code-smell-pr-coverage-2026-10-04.json)包含 workspace、各 crate 和 334 个源码文件的行统计。范围为上述源码提交的 Rust workspace、默认 features；使用工具默认过滤，无额外文件排除。未覆盖 TypeScript、Kotlin；3 项需要外部 Provider 的测试 ignored。本地 HTML 报告位于 `target/llvm-cov/html/index.html`。

[之前的可比 workspace 测量](cargo-workspace-pr-validation-2026-10-04.md)为 49,024 / 51,879（94.50%），相同平台、工具链和默认 features。显示到小数点后两位的覆盖率变化为 0.00 个百分点；两次测量之间还有其他源码提交，不能将差异单独归因于本次改动。

本次触及的 `crates/provider/src/local/opencode/runtime.rs` 为 158 / 205 行（77.07%），`crates/metadata/src/local/workspace_automation/retirement.rs` 为 51 / 65 行（78.46%）。后续可补充 OpenCode 运行时错误事件和 workspace 退役恢复路径的测试；需要真实 Provider 的行为仍需在有认证的环境中验证。
