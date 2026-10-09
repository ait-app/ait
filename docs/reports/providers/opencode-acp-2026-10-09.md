# OpenCode 官方 ACP 迁移验证

2026-10-09；源码提交 `007220f5d5f2ba74bd0e3de8ba6c42c8cb3e7283`，基于 `938a98f548e216cacf9b7caecee62383d4a27442`。
边界决策见 [ADR-115](../../decisions/providers/adr-115-opencode-acp-provider.md)。

OpenCode 使用官方 `opencode acp`，移除私有 HTTP/SSE 与本地累计 token 门禁。
1.x / 2.x 自动使用各自原生配置格式；模型、模式和会话能力来自当前原生响应。
界面不展示版本能力比较或兼容模式，不为缺失能力补造另一版的功能。
会话控件使用本会话的原生模式，避免静态模式或其他项目目录污染当前会话。

原生认证、工具和历史仍由 OpenCode 所有；不改写用户配置、数据库或本机安装。
审批、原生表单、取消和历史重放接入已有 UI；模糊接纳或失败不会自动重发 prompt。
完整工具结果留在原生 transcript，展示预览限制为 32 KiB，文本按 96 KiB UTF-8 边界分段，保留 JSON 转义余量。
有 delete 时查询与摘要临时会话结束后删除；无 delete 时不创建辅助会话，1.x 目录来自其原生只读 CLI。
缺少可选能力只限制对应功能；尚无表单回调的原生版本不启用无法回答的 question。

## 验证

- Workspace：2004 passed / 0 failed / 14 ignored，含 1 doctest。
- ACP 回归：31 passed；会话控件：8 passed。
- 隔离 XDG 与 loopback 模型：原生 1.18.4 的 3 项和 2.0.26 的 4 项通过；不读取用户认证或调用付费模型。
- Workspace build、严格 Clippy、fmt、文档链接与 diff 检查通过。
- 早期 2.0.20 的独立并发原生验证曾在创建会话时遗漏配置模型，原因尚未确认；没有通过重发 prompt 掩盖。显式选择未被原生目录提供的模型时，在 prompt 前拒绝。

## Test coverage

Workspace **94.70%（55,890/59,018）**；provider **94.29%（26,444/28,045）**。
OpenCode 生产实现 **92.20%（2,115/2,294）**；共享 ACP transport **89.86%（186/207）**。
无同一基准提交、同一范围的可比测量。
共享逐文件计数、源码指纹、精确命令与未覆盖行见 [覆盖率证据](opencode-acp-coverage-2026-10-09.json)。

测量使用 macOS arm64、Rust 1.98.1、默认 features，无额外 workspace 文件排除。
执行 `CARGO_TARGET_DIR=/private/tmp/ait-opencode-acp-coverage CARGO_BUILD_JOBS=6 nix develop --command cargo llvm-cov --workspace --html -- --test-threads=1`，
追加 JSON 中的 `coverage_native_v2` / `coverage_native_v1` 命令，再执行 `cargo llvm-cov report --html` 与 `report --json --summary-only`。
Instrumented workspace：2003 passed / 0 failed / 14 ignored；随后显式运行 35 项 ACP 测试（含 4 项 2.0.26 原生测试）和 3 项 1.18.4 原生测试。
默认 ignored 的 5 项 OpenCode 测试均按原生版本执行，其余 9 项外部安装测试未执行。
Doctest、Python fixture 和上游 binary 不在 Rust instrumentation 范围。
HTML 本地路径：`/private/tmp/ait-opencode-acp-coverage/llvm-cov/html/index.html`；共享证据为上述 JSON。

Windows、付费模型、真实进程丢失、删除重连失败和关闭结算超时未验证；后续需故障注入覆盖。
[此前 Linux CI](https://github.com/ait-app/ait/actions/runs/37906169356/job/113740083680) 在 `4ac01946` 通过；当前兼容性修订由 PR CI 单独验证。
此前 Linux 关闭测试改用进程状态区分已退出 zombie 与可运行进程，保留两秒停止检查，仅修改测试。
