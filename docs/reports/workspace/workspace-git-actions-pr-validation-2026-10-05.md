# 工作区重置与 Git 主操作：PR 验证

源码提交：`98bb2970dadf6df2894a465338e8f21a5fb91a60`。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。Cargo 命令使用默认 features 和离线模式，`SHERPA_ONNX_LIB_DIR` 指向本机缓存的 sherpa-onnx v1.13.8 静态库。

## 变更范围

- “从 main 更新”改为“重置工作区”：先 fetch 最新的 `origin` 默认分支（`main` 或 `master`），恢复工作区创建时的分支名，再将该分支的 HEAD 重置到最新默认分支。操作提供清楚的覆盖本地改动提示。边界决定见 [ADR-083](../../decisions/workspace/adr-083-reset-workspace-to-origin-default.md)。
- 主按钮根据工作区改动、远程分支和 PR 状态显示 Git 操作。已有 PR 的后续更新使用普通 push；删除同名分支校验及独立的“更新 PR”操作。边界决定见 [ADR-084](../../decisions/workspace/adr-084-workspace-primary-git-action.md)。
- checkout 与 forge 状态增加分支和提交信息，配套更新协议、客户端和移动端测试。

## 构建、测试与静态检查

| 命令或范围 | 结果 |
| --- | --- |
| `cargo llvm-cov --workspace --html --offline` | 通过；1,797 passed，0 failed，3 ignored |
| `cargo build --workspace --offline` | 通过 |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | 通过 |
| `cargo fmt --all -- --check`、`git diff --check` | 通过 |
| 移动端 Git 操作、状态和文案相关测试 | 126 passed |
| 移动端 Rust daemon transport 测试 | 29 passed |
| 客户端 daemon-client 测试 | 141 passed |
| 协议 checkout/PR schema 测试 | 38 passed |
| `npm run build --workspace=@ait/client` | 通过 |
| 变更的 TypeScript 文件 `oxfmt --check`、`oxlint` | 通过 |
| `node scripts/check-docs.mjs`、`python3 scripts/check-paseo-client-methods.py` | 通过 |

移动端全量 `tsgo --noEmit` 未通过：当前本地依赖树缺少 `expo-clipboard`、`react-native-keyboard-controller`、`htmlparser2`、`@xterm/xterm` 等包；输出中没有本次变更文件的报错。安装完整移动端依赖后仍需重跑全量类型检查。

## Test coverage

完整 Rust workspace 测试在 `cargo llvm-cov --workspace --html --offline` 中执行通过。测试结果为 1,797 passed、0 failed、3 ignored；这与下面的行覆盖率分别统计。

| 范围 | 已覆盖 / 总行数 | 行覆盖率 |
| --- | ---: | ---: |
| Rust workspace | 49,427 / 52,321 | **94.47%** |
| filesystem | 11,338 / 11,959 | 94.81% |
| api | 1,975 / 2,134 | 92.55% |
| metadata | 7,541 / 7,955 | 94.80% |
| daemon | 922 / 967 | 95.35% |

[可审查的覆盖率产物](workspace-git-actions-pr-coverage-2026-10-05.json)包含 workspace、各 crate 和 338 个源码文件的行统计。测量范围为上述源码提交的 Rust workspace、默认 features；使用工具默认过滤，无额外文件排除。本地 HTML 报告位于 `target/llvm-cov/html/index.html`。

[先前同平台和工具链的 workspace 测量](../daemon/rust-code-smell-pr-coverage-2026-10-04.json)为 49,035 / 51,889（94.50%）；本次为 94.47%，下降约 0.03 个百分点。两次测量之间包含主分支更新及功能改动，不能将差异单独归因于某一项变更。

尚未在真实远程仓库上做完整的重置工作区与 PR 更新端到端验证；现有测试覆盖本地 Git 仓库、RPC/协议和操作状态。后续可在集成环境补充远程 fetch、push 和 PR 状态变化的完整流程。
