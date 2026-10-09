# 草稿重试、文件类型与 OpenCode：验证

## 行为变更

1. **草稿重试身份**：每次新的手动提交生成新的 message / idempotency key；继续同一次尝试保留原身份。保留运行中的创建结果供重挂载观察，修改配置后不再重放旧失败请求。
2. **模式图标**：OpenCode Build 显示 Hammer，未知或缺省模式统一使用 Bot 回退，避免菜单项无图标。
3. **PDF 类型识别**：包括 `.PDF` 在内的 PDF 扩展名返回 Binary / application/pdf，避免 ASCII 头 PDF 被当作文本。
4. **OpenCode 错误与并发回归**：保留静态错误原因并记录 HTTP 状态、历史变更失败，端口错误分类不变；增加已安装 OpenCode 的兄弟会话关闭回归测试。

## Test coverage — 2026-10-09

基线 `f7f434a5`，受测实现树 `0f67a96006a5ad334a7c5ad24c76e279dd72a209`。
Linux x86_64、Rust 1.98.1、LLVM 23.1.1，workspace 默认特性。

- `cargo test --workspace`：2,044 passed / 16 ignored，包含 doc tests。
- `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`：通过。
- 下列前端 7 文件测试：36 passed；typecheck、oxlint 和文档链接检查通过。
- `AIT_TEST_OPENCODE_BIN=/usr/bin/opencode cargo test -p provider installed_opencode_keeps_sessions_writable -- --ignored`：1 passed，隔离配置和 loopback 模型。

Rust 测试使用空 `XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config` 隔离 Git 全局 hook 设置。
覆盖率命令：

```sh
XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
CARGO_TARGET_DIR=target/diagnostics-coverage \
cargo llvm-cov --no-clean --workspace --html -- --test-threads=4
```

首次插桩运行的 DSH 测试 `native_argument_validation_failures_do_not_fail_the_adapter_or_lose_user_messages`
在创建夹具会话时返回 `Unavailable`。保留采样数据完整复测后，2,043 passed / 16 ignored，0 failed。
此次未改动 DSH 实现；失败原因尚未确定。覆盖率不含 doc tests，前端覆盖率未测量。

| Rust 行覆盖范围 | Covered / total | 行覆盖率 |
| --- | --- | --- |
| Workspace | 57,093 / 60,604 | 94.21% |
| Filesystem | 11,570 / 12,198 | 94.85% |
| Provider | 27,646 / 29,631 | 93.30% |

[本次覆盖率摘要](draft-files-opencode-coverage-2026-10-09.json)包含完整命令和修改文件统计。
HTML 可在 `target/diagnostics-coverage/llvm-cov/html/index.html` 重建。
没有同一源码基线的对照测量，不计算覆盖率增量。未验证其他平台、可选特性及真机 UI。

## Test coverage — 2026-10-08 历史结果

受测实现树：`481cfaffd6814361718010c32865074fe65e7fe0`（文档整理前的实现树）。
环境：Linux x86_64，Rust 1.98.1，LLVM 23.1.1。无同范围 main 基线测量，不声称覆盖率增量。

```sh
XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config cargo test --workspace
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
XDG_CONFIG_HOME=/tmp/ait-diagnostics-test-config \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
CARGO_TARGET_DIR=target/diagnostics-coverage \
cargo llvm-cov --workspace --html -- --test-threads=4
```

独立空 XDG 目录隔离本机全局 Git hooksPath，未修改用户配置。所有命令通过。
普通测试：**1,999 passed / 16 ignored**，包含 doc tests；覆盖率运行：**1,998 passed / 16 ignored**，不含 doc tests。

| Rust 行覆盖范围 | Covered / total | 行覆盖率 |
| --- | --- | --- |
| 默认特性 workspace | 55,827 / 59,291 | 94.16% |
| filesystem | 11,567 / 12,192 | 94.87% |
| provider | 26,796 / 28,758 | 93.18% |

[覆盖率摘要](draft-files-opencode-coverage-2026-10-08.json)包含命令、逐 crate 和修改文件数据。
完整 HTML 在本机 `target/diagnostics-coverage/llvm-cov/html/index.html`，可由上述命令重建。
默认忽略的真实 Harness 测试不计入此覆盖率。另行执行真实 OpenCode 并发测试：

```sh
AIT_TEST_OPENCODE_BIN=/usr/bin/opencode cargo test -p provider installed_opencode_keeps_sessions_writable -- --ignored --nocapture
```

**1 passed**，使用隔离配置和 loopback 模型，无在线模型请求。
定向 Rust 测试：`cargo test -p filesystem local::files` 为 36 passed；
`cargo test -p provider local::opencode::client` 为 10 passed / 1 ignored。

前端两次 unit 测试共 **7 个文件、36 项通过**：

```sh
npm run test --workspace=@ait/mobile -- --project unit src/composer/draft/create-flow.test.ts src/stores/workspace-draft-submission-store.test.ts src/composer/agent-controls/utils.test.ts
npm run test --workspace=@ait/mobile -- --project unit src/composer/draft/workspace-tab.test.ts src/file-pane/binary-preview.test.tsx src/agent-controls/labels.test.ts src/agent-controls/policy.test.ts
npm run typecheck --workspace=@ait/mobile
```

修改的 TypeScript 文件通过 oxlint 与 oxfmt；文档链接与 `git diff --check` 通过。
前端行覆盖率未测量；已执行行为回归与类型检查，未执行真机 UI 交互。
