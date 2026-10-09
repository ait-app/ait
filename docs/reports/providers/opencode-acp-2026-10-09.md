# OpenCode 官方 ACP 迁移验证

2026-10-09；基于 `938a98f5`，分支 `refactor/opencode-acp`。
测量源码提交：`b3cb2a64f7017d4ad77982e7699ea259686c771b`；后续仅更新验证文档，不改变测量的 Rust 源码。
边界决策见 [ADR-115](../../decisions/providers/adr-115-opencode-acp-provider.md)。

## 实现范围

OpenCode 从私有 HTTP/SSE 切换为官方 `opencode acp` 子进程，要求 OpenCode 2.x 的 2.0.26+。
旧版不再进入缺少原生问答回调的执行路径，也没有旧协议回退。
原生 session ID、认证、工具和历史仍属于 OpenCode；Ait 不修改原生数据库，也不向用户项目写入脚本或配置。
旧 Ait 句柄的 native ID 与 client message 映射可继续读取；跨 OpenCode 存储版本的恢复由 OpenCode 自己负责。

审批保留原生 once / always / reject 选项，问答保留表单字段 key、原生选项值、显示标题、多选与自定义输入。
回答验证后只发送一次；取消等待原生终态与持久化重放，才放行下一次输入。
模糊接纳、连接错误与重放失败不会自动重新提交模型输入。
模型、模型专属 effort 和 primary agent 从 ACP config options 发现，临时查询会话没有 prompt 并在结束后删除。
摘要使用独立、无工具、单步会话，正常结束与 future cancellation 均清理原生历史。

完整原生工具输出仍在 OpenCode transcript；Ait 保存 32 KiB 展示预览，文本按 128 KiB 的 UTF-8 边界分段。
完整历史通知增量消费，不受控制通知队列 128 项的限制。
会话列表以 4 路并发、单项 3 秒、整体 10 秒的预算补齐首尾用户输入预览；失败只省略预览。
共享 ACP stdio transport 复用现有 DSH profile 的进程所有权与控制机制，DSH 默认 native Host 保持原有行为。

## 验证

真实 CLI 使用官方 OpenCode 2.0.26，隔离 XDG 目录和确定性的本机 loopback 模型。
测试不读取用户认证，不调用真实模型服务，不替换用户安装，不中断正在运行的原生会话。
离线 ACP fixture 是仓库内不可变可执行文件，所有可写状态留在每项测试的临时目录。
并发测试直接复用该可执行文件，不在 fork/exec 前改写脚本。

首次 [Linux CI](https://github.com/ait-app/ait/actions/runs/37900784280/job/113722621686)
在 GUI workspace 创建测试的关闭检查中报告一个 helper PID 仍存在；另两项 OpenCode 进程测试通过。
原断言使用 `kill -0`，无法区分运行中进程与已退出但尚未被新父进程回收的 zombie，日志也没有记录该 PID 的状态。
测试现与既有 Codex shutdown 验证一致，以 `ps` 的进程状态判断是否已停止，并给信号处理最多 2 秒。
不存在或 zombie 表示没有继续执行工作；仍可运行的进程继续使测试失败并打印其状态。
该修正只属于测试，没有改动生产执行策略，也没有取消进程停止验证。

最终命令、测试计数、源码指纹与逐文件覆盖率见 [覆盖率证据](opencode-acp-coverage-2026-10-09.json)。

- `cargo test --workspace`：1996 项通过、0 失败、13 项 ignored，包含 1 项 doctest。
- OpenCode ACP 定向验证：28 项通过，包含 4 项真实 OpenCode 2.0.26 测试。
- daemon 的 3 项 OpenCode 进程集成测试通过，覆盖外部导入、重启恢复与继续输入。
- `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。
- 文档链接检查与 `git diff --check` 通过。

命令均通过 `nix develop --command` 使用仓库固定工具链。测试执行计数与下面的覆盖率分别报告。

## Test coverage

当前合并测量：workspace **94.67%（55,664/58,798）**，provider **94.22%（26,218/27,825）**。
OpenCode ACP 生产实现 **91.07%（1,898/2,084）**，共享 ACP transport **89.86%（186/207）**。
没有同一基准提交、同一测试范围的可比测量；此前 HTTP adapter 的结果不作为本次 ACP 基线。
HTML 仅在本机生成；可共享的逐文件计数和源码证据保存在上面的 JSON 文件。

执行 `CARGO_TARGET_DIR=/private/tmp/ait-opencode-acp-coverage nix develop --command cargo llvm-cov --workspace --html -- --test-threads=4`
后，使用同一 target 下的 `cargo llvm-cov --no-report -p provider --lib -- local::opencode --include-ignored`
显式加入 28 项 ACP 测试，再执行 `cargo llvm-cov report --html` 和 `cargo llvm-cov report --json --summary-only`。
完整 instrumented suite 为 1995 项通过、13 项 ignored；真实 CLI 的 4 项在后续定向命令中执行。
完整命令和二进制路径见 JSON；没有额外的 workspace 文件排除，doctest、Python fixture 和上游 OpenCode binary 不在 Rust instrumentation 范围内。

macOS arm64 使用固定 Rust 1.98.1 与默认 Cargo features；Linux 结果由 PR CI 单独记录。
Windows 与真实付费模型未验证。默认 workspace suite 中依赖外部安装的测试保持 ignored，真实 OpenCode 的 4 项另行显式运行。
ACP 缺少逐消息时间戳和完整外部 writer 状态，导入使用观察时间，既有 timeline 身份保留首次持久化时间。
关闭或清理超时、失联原生进程等故障分支必须明确返回失败，不能当作成功恢复。
未覆盖的重点包括真实进程丢失、删除临时会话的重连补救失败，以及关闭时超过结算期限。
这些需要后续故障注入验证；正常路径的 native 清理与 future cancellation 已验证。

重跑覆盖率时，一次与普通全量测试同时执行的默认并发运行在未修改的 directory_sync WebSocket 测试中超时；普通全量测试通过，覆盖率随后单独以 4 个测试线程完整重跑通过。没有修改或跳过该测试。
