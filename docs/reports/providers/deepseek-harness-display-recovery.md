# DSH 内部上下文与展示缓存重复修复

后续固定 `4a706e5f` 桌面复测确认：上下文过滤通过，重复回复仍复发。
第二条原因及后续修复见[缓存重复加载报告](deepseek-harness-cache-recurrence.md)。
以下验证只对应本阶段，不代表桌面验收通过。

日期：2026-10-04。PR #169；测量基于 `3be0648d40c6736a91fb9b832783b548754c1c73`
加本次源码修改，[覆盖率附件](deepseek-harness-display-coverage.json)记录精确源码 SHA-256。

## 复测证据与根因

外部测试者使用固定提交 `aa438db5`、实际 0.0.16 桌面及真实 DSH，确认旧用户气泡恢复、
空工具参数和非法 JSON 继续进入原生工具校验；问答、审批、取消和重启续聊通过。
该轮仍发现内部运行上下文气泡及末尾回复重复，因此不是完整验收通过。

- **内部上下文**：官方 DSH 0.1.5-rc.2 的 `dsh-agent-loop` 以
  `source.kind: "plugin"`、`plugin: "@deepseek-ai/dsh-system-prompt"` 写入运行上下文；
  `user/message` 事件名本身不能证明是真人输入。实时和历史共享投影现在只接受
  `source.kind == "user"`，忽略其他来源的展示内容和附件读取。
  原子历史 reconcile 会移除旧 Ait 数据库里已误投影的上下文，不修改 DSH 原生记录。
  测试使用与上下文完全相同的真人文本，确认没有文本前缀过滤。
- **重复回复**：生产客户端 owner + store 回归复现了 `range: null` 展示缓存被恢复后，
  第一份完整 tail 响应走增量拼接、将一条 `Local answer 1 complete` 变成两份的情况。
  修复将无权威游标的首个 tail 作为替换快照，并保留更新的 live head。
  新增展示投影版本，使旧客户端缓存一次性失效并从 daemon 重新读取，避免已经重复的缓存
  带着有效游标被永久保留。只失效可重建的 timeline 缓存，目录缓存、daemon 数据库和原生会话保留。

这证明了客户端可产生报告中的重复现象；未使用测试者的原始数据库，修复版桌面仍需独立复测。
没有依据认为模型被重复执行。本次共享客户端修改涉及缓存和历史归并，不改变 Codex、OpenCode
或 DSH 的原生执行协议、模型调用和权限行为。没有引入新领域边界。

## 测试执行

- DSH native + ACP 定向：**30 passed、0 failed、1 ignored**。
  新测试覆盖插件/goal/未知/缺失来源过滤、同文真人消息保留、内部附件不读取、
  live 与分页恢复一致、旧错误记录清理、SQLite 重开与重复 reconcile 不换 epoch、不重放 prompt。
- 客户端缓存和 replica：**64 passed**，含连续两次刷新、三次重开及旧投影缓存失效。
  扩展 timeline/stream/cache 测试：**303 passed、3 failed**；本次新增回归均通过。
  另外覆盖空权威历史清理旧显示、两条内容相同但身份不同的真实回答、新 live head 保留。
- 3 项失败已在未修改的 `3be0648d` reducer 上复现；`stream.ts` 和相关旧用例也与该提交一致。
  它们检查插件行身份，而现有 `streamTimelineItemIdentity` 只返回工具身份：
  `uses the protocol identity format for stream tool and plugin rows`、
  `replaces a live row when the plugin-scoped identity repeats`、
  `keeps the newest plugin row across the older-page prepend boundary`。
  本次没有修改插件行实现，也没有删掉或跳过这些失败测试；不把扩展测试宣称为全绿。
- 移动/Web 全量类型检查、变更 TypeScript 文件 oxfmt/oxlint 通过。
- Rust fmt、workspace clippy（all-targets、`-D warnings`）、build 通过。
  普通 workspace 测试 **1787 passed、0 failed、4 ignored**；覆盖率运行同样通过。
- 文档链接与 release 版本一致性检查通过。
  `aa438db5` 的旧 Rust CI 问题已由 `3be0648d` 修复，详见[独立 CI 修复报告](../daemon/initial-prompt-ci-validation.md)。

命令（Rust 在 Nix shell 内设置共享 `CARGO_TARGET_DIR`）：

```sh
cargo test -p provider local::deepseek_harness --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline -- --test-threads=1
cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-dsh-display-coverage.json
npm run test --workspace=@ait/mobile -- src/timeline/replica.test.ts src/runtime/replica-cache --project unit
npm run test --workspace=@ait/mobile -- src/timeline src/types/stream.test.ts src/runtime/replica-cache --project unit
npm run typecheck --workspace=@ait/mobile
```

## Test coverage

| Rust 范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Workspace | 50,391 / 53,398 | 94.37% |
| Provider crate | 22,666 / 24,192 | 93.69% |
| DSH（native + ACP） | 2,309 / 2,513 | 91.88% |
| DSH native Host | 1,300 / 1,439 | 90.34% |

Workspace 相对同平台 `3be0648d` 的 50,385 / 53,391（94.37%）变化 **+0.00 个百分点**；
native Host 相对 `aa438db5` 的 1,293 / 1,432（90.29%）变化 **+0.05 个百分点**。
`3be0648d` 仅改变 daemon 测试，该 native Host 基线可比。

平台 aarch64-apple-darwin，Rust 1.98.1，cargo-llvm-cov 0.8.7；默认 features、默认过滤，
无额外文件排除；doctest 不计入行覆盖率。4 项忽略测试包括既有 CLI/在线提供方测试，
本次没有重跑真实 DSH 无模型烟雾测试。前端行覆盖率未测量，测试数不等于覆盖率。

[JSON 附件](deepseek-harness-display-coverage.json)包含 workspace/逐 crate/native 文件指标、
源码与日志哈希及基线。HTML 位于共享 target 的 `llvm-cov/html/index.html`。
仍未完全覆盖部分原生启动、异常退出、畸形 RPC、历史大小上限等错误分支。
本次未完成真实模型/Electron、Debian amd64、Windows 或原生移动端验收。

## 固定提交桌面交接

从 PR #169 的本次固定修复提交同时构建 daemon 与桌面，记录两者 SHA 和 DSH 版本。
保持默认 native Host；不要使用旧 aa438db5 安装包，也不要设置 ACP 兼容开关。

1. 打开截图对应的旧会话，不发新 prompt，确认真人用户气泡和助手/工具顺序保留，
   内部运行上下文气泡消失；检查 Ait 数据库旧错误展示记录被清理、DSH 原生记录保留。
2. 保留旧客户端缓存升级，连续刷新、切换工作区、完整重启至少三次；末尾回答始终只有一份。
   同时核对 daemon 历史与原生模型请求计数，确认没有额外请求。
3. 新会话测试同文连续回答、工具后回答、取消后继续、加载旧分页；网络恢复期间有 live 输出时
   不应重复或丢失新内容。可用明确提示让真人输入包含 `Current runtime context`，确认不会被误过滤。
4. 回归单选/多选/自定义回答，允许/拒绝/只读模式，空参数/非法 JSON 的工具校验及重启续聊。
5. 因共享缓存恢复修复，抽测 Codex、Claude、OpenCode 的刷新/重启/正在输出时重连。

报告请附固定 SHA、系统/架构、模型来源、刷新前后截图和脱敏失败日志。外部复测通过前不合并 PR。
