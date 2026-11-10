# Paseo Server PR 提交前验证

## Test coverage

测量基线为 `b797e0d2f83ae57f62892c288a5d81776f8afa6a` 加本 PR 的修改；源码 SHA-256、逐文件未覆盖行和测试日志指纹保存在
[覆盖率证据](paseo-server-pr-coverage-2026-09-29.json)。
相对[最近同口径历史测量](../clients/desktop-speech-terminal-coverage.json)的 91.27%（36,250 / 39,716），增加 0.43 个百分点。
该历史测量对应 47d4b7ee 加当时修改；未对精确 PR 基线重跑，因此不把差值归因于本 PR 单独产生的效果。

| 测量口径                            | 覆盖率 | 已覆盖 / 总行数 |
| ----------------------------------- | -----: | --------------: |
| 完整 Rust workspace（LLVM summary） | 91.71% |   40580 / 44250 |
| 改动生产文件整体（LLVM summary）    | 91.63% |   17820 / 19448 |
| 新增或修改可执行行（LCOV DA）       | 95.16% |     4658 / 4895 |

LLVM summary 与 LCOV DA 的可执行行统计口径不同，上表分别标明，不混合两者分母。
macOS arm64、默认 features；使用 cargo-llvm-cov 默认文件过滤，没有自定义文件排除。
默认未插桩 doctest；Linux/Windows、真实账号 Provider 推理和远端 forge/push 未运行。
3 项既有在线 Provider 测试保持 ignored。原始 profile、日志和 HTML 不提交。

| crate             | 覆盖率 | 已覆盖 / 总行数 |
| ----------------- | -----: | --------------: |
| bins/daemon       | 94.36% |       870 / 922 |
| crates/api        | 95.54% |     1627 / 1703 |
| crates/browser    | 97.38% |       707 / 726 |
| crates/domain     | 100.0% |       121 / 121 |
| crates/filesystem | 88.22% |     8355 / 9471 |
| crates/metadata   |  90.7% |     6862 / 7566 |
| crates/model      | 95.45% |       629 / 659 |
| crates/protocol   | 100.0% |         57 / 57 |
| crates/provider   | 93.23% |   17346 / 18606 |
| crates/schedule   |  97.8% |       802 / 820 |
| crates/terminal   | 91.06% |     1549 / 1701 |
| crates/voice      |  87.2% |     1655 / 1898 |

## 测试与静态检查

- 普通完整 Rust 测试：1460 通过，3 ignored，0 失败。
- 覆盖率运行：1460 通过，3 ignored，0 失败；不是新增测试数量。
- SDK adapter 和真实本地 server 集成测试共 44 项通过；SDK creation 9 项通过。
- App 全量 TypeScript 检查通过；本地缺失的 highlight workspace 链接已恢复，未修改相关业务源码。
- 205 项协议索引校验、45 项原始上游测试及 3 个 locale 排序数据核对通过。
- cargo fmt、workspace build、Clippy -D warnings、修改 JS/TS 的 oxfmt/oxlint、Python 语法及 git diff --check 通过。

## 复现

```sh
cargo test --locked --offline --workspace
cargo llvm-cov clean --workspace
cargo llvm-cov --locked --offline --workspace --html
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-paseo-pr-workspace-summary.json
cargo llvm-cov report --lcov --output-path /tmp/ait-paseo-pr-workspace.lcov
cargo build --locked --offline --workspace
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm run build:ui-deps
npm run typecheck --workspace=@ait/mobile
AIT_TEST_RUST_SERVER="$PWD/target/debug/daemon" npm run test --workspace=@ait/mobile -- src/runtime/rust-daemon/
```

HTML 位于 `target/llvm-cov/html/index.html`，可评审的行数、未覆盖行和源码指纹由上述 JSON 提供。

## 限制与后续

仍需补充持久化/队列故障注入及操作系统拒绝终止进程后的清理责任验证。
真实账号、远端 forge/push 和其他平台须在对应环境验证。全局会话事件、全局 hook、
失败创建恢复等兼容差异，以及 App 适配层与上游的关系，见[API 审计](paseo-api-audit-2026-09-29.md)。
此前的[定向覆盖率](paseo-server-coverage-2026-09-29/README.md)保留为实现阶段证据，口径不同，不与本次百分比混用。
