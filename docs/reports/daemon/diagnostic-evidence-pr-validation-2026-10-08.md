# 本地诊断证据：PR 验证

日期：2026-10-08。平台：Linux x86_64，Rust 1.98.1，LLVM 23.1.1。
分支：`feat/diagnostic-evidence`，基线：`4794dbc4`。
受测实现树：`9430cf0a35a4b0e00b2aec684e08c1b9ab72209e`（加入本报告前的 Git tree）。

## 范围

诊断采集包含有界错误事件、原生版本和时间窗口日志、脱敏、证据保留、手动诊断端口、
前端文本下载与移动端分享。metadata 服务通过诊断端口调用采集器，证据持久化归 file crate。

## 验证

- `cargo test --workspace`：2,011 passed，15 ignored，0 failed（含 doc tests）。
- `cargo build --workspace`：通过，无编译警告。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`：通过。
- `npm run test --workspace=@ait/mobile -- --project unit src/diagnostics/app-diagnostic-report.test.ts src/diagnostics/desktop-diagnostic-report.test.ts src/diagnostics/export-diagnostic-report.test.ts src/i18n/resources.test.ts`：4 个文件、48 项通过。
- `npm run typecheck --workspace=@ait/mobile`：通过。
- `npx oxlint` / `npx oxfmt --check`：修改的诊断组件、诊断逻辑和翻译文件通过。
- `npm run check:docs`、`git diff --cached --check`：通过。

Rust 普通构建使用 `CARGO_TARGET_DIR=target`。
测试额外设置 `XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config`，该目录为空：本机全局
`core.hooksPath=.githooks` 会覆盖现有 Git 重置测试的临时 hook，造成两项断言失败。
代码清理 GIT_* 子进程环境，因此仅设置 `GIT_CONFIG_GLOBAL` 不能隔离此配置。
使用独立 XDG 配置后，16 项重置测试和完整 workspace 均通过；没有修改用户 Git 配置。

## 覆盖率

命令：

```sh
XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
CARGO_TARGET_DIR=target/diagnostics-coverage \
cargo llvm-cov --workspace --html -- --test-threads=4
```

结果：2,010 passed、15 ignored、0 failed；默认特性的 Rust 单元／集成测试，不含 doc tests。
Workspace 行覆盖率 **56,412 / 59,921 = 94.14%**，函数覆盖率 93.27%。
新增实现模块：daemon 251/272 = **92.28%**、file 65/68 = **95.59%**、provider 220/234 = **94.02%**。

[可分享的覆盖率 JSON](diagnostic-evidence-coverage-2026-10-08.json) 包含总量、逐 crate
行数和改动文件摘要；由 `cargo llvm-cov report --json --summary-only` 导出并整理。
完整 HTML 位于本机 `target/diagnostics-coverage/llvm-cov/html/index.html`，
可用上述命令重建；没有将庞大构建产物或运行日志提交到仓库。
首次默认并发插桩运行中，现有
`failed_queued_admission_does_not_block_an_independent_agent` 测试观察到 Closed 而非 Error；
普通全量测试通过。随后降低测试并发至 4 完整复测通过，此次失败保留在验证记录中。

## 已测行为与限制

测试覆盖时间边界及 UTC 偏移、目录／文件大小限制、符号链接与会话文件排除、CLI 超时及
不可用状态、错误合并和队列饱和、脱敏、持久化故障、重启后的证据读取，以及保留期限和容量。
前端测试使用模拟 Web 和 Expo 接口验证下载／分享、不可用状态、失败路径及临时文件清理。

未执行真实 Android/iOS 分享面板、打包 Electron 下载和各 Harness 在线生产会话的人工验证。
Linux 默认 workspace 覆盖率不代表其他平台／可选 feature；15 项 ignored 测试没有执行。
无基线覆盖率测量，不能据此声称覆盖率增量。自由文本脱敏是尽力而为，非形式化隐私保证。
操作与时效限制见 [故障诊断](../../operations/diagnostics.md)。
