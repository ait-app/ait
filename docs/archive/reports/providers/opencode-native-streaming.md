# OpenCode 原生流事件与 Workspace 响应修复

> 历史验证记录：下述路径、命令和指标对应所注明的源码提交；当前目录与上游整合结果见 [PR 验证](opencode-upstream-pr.md)。

验证日期：2026-10-01 UTC。基线：`e996f9c1aba4cba232a640b2446c8bb8d1c10ce7`。
本报告继续处理外部复测发现的三个阻断；此前模型 provider 与恢复权限修复仍保留。

## 问题与修复

| 问题                                             | 确认的原因                                                                                                                     | 本次修复                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1.18.33 普通文本轮次报 Provider execution failed | 原生 text-start 发布合法空文本 part，Ait 使用拒绝空字符串的 required_string；且没有处理 message.part.delta                     | 专用 Stream 处理空占位、原生增量和累计快照。空文本不产生进度事件，缺字段/非字符串仍拒绝；累计快照只发布尚未显示的后缀                               |
| 用户文本成为 assistant_message                   | part.updated 未关联父消息角色，Bridge 将全部 TextDelta 作为助手回复                                                            | 维护 message.updated 的角色映射，只发布已确认助手的文本；未知角色的文本有界缓冲，确认用户后丢弃。当前输入预先标记为用户；不猜测未知 part 的内容类型 |
| workspace.create.response 被前端拒绝             | OpenCode persistence.nativeHandle 返回对象，正式 AgentPersistenceHandleSchema 要求 string；根 agent 与 creation.agent 均受影响 | 插件将内部恢复数据编码为不透明 JSON 字符串。恢复与只读历史兼容旧对象句柄，拒绝无效/过大的编码；前端 schema 保持原约束                               |

流事件物化仍在 Rust OpenCode 插件内部，未改变 domain 或 application 边界。
缓冲限制复用执行的输出字节和步骤限额；超限仍中止原生执行。
推理/其他 part 的 delta 不猜测为普通文本；乱序的角色元数据到达后再发布确认的助手内容。
最终历史仍通过现有原生历史投影落库，流式和最终 key 保持统一。

参考原生 [1.18.33 processor](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/session/processor.ts)
的 text-start → delta → text-end 序列，以及 Paseo 提交
`5599f9e567128a1240b3b15afab28bceef9d36a5` 的 OpenCode 角色关联与文本物化方式。

## 实际运行验证

官方 npm 发行的 macOS arm64 OpenCode 可执行文件分别为 1.18.33、1.14.46；
运行当前源码编译的 Ait server，经 WebSocket RPC 使用隔离临时项目和 XDG 目录。
模型是本机确定性 OpenAI-compatible SSE 服务，只启用 localtest provider，
未使用用户凭据、外部推理模型或付费额度。实际响应为 `Local deterministic answer.`。

| 用例                                                                 | 1.18.33    | 1.14.46    |
| -------------------------------------------------------------------- | ---------- | ---------- |
| 模型发现与正式 ProviderSnapshotEntrySchema                           | 通过       | 通过       |
| workspace.create + initialPrompt，正式 WorkspaceCreateResponseSchema | 通过       | 通过       |
| 同一会话前两轮文本与 idle 完成                                       | 通过       | 通过       |
| 持久时间线恰有两条用户消息，助手内容不包含用户输入                   | 通过       | 通过       |
| 停止/重启 Ait server，最新句柄恢复                                   | 通过，1 次 | 通过，1 次 |
| 恢复响应正式 AgentSnapshotPayloadSchema 与第三轮文本                 | 通过       | 通过       |

两版探针均执行三轮文本、一次重启恢复；各有四次本地模型请求，包含原生后台请求。
正式 schema 从仓库 `packages/protocol/src/messages.ts` 原样打包，使用锁文件中的
Zod 4.4.3、semver 7.7.4、esbuild 0.27.3；未放宽 schema。

另做单变量响应诊断：同一已通过 schema 的真实创建响应，只在内存将
`agent.persistence.nativeHandle` 或 `creation.agent.persistence.nativeHandle` 转回对象，
分别复现 `invalid_type / expected string / received object`，错误路径对应上述两个字段。
这确认了创建响应契约问题；原生 Electron 窗口完整创建/对话流程本轮未重跑，
不能把 RPC + 正式 schema 通过写成 GUI 端到端通过。

命令和临时证据入口（不提交凭据、运行数据库或原始 RPC 记录）：

```sh
node /private/tmp/ait-opencode-test-tools/real-probe.mjs \
  /private/tmp/ait-opencode-test-tools/node_modules/opencode-darwin-arm64/bin/opencode \
  /private/tmp/ait-opencode-real-1.18.33
node /private/tmp/ait-opencode-test-tools/real-probe.mjs \
  /private/tmp/ait-opencode-1.14-test-tools/node_modules/opencode-darwin-arm64/bin/opencode \
  /private/tmp/ait-opencode-real-1.14.46
node /private/tmp/ait-opencode-test-tools/check-workspace-contract.mjs
```

## Test coverage

- 修复前先补回归：4 passed、3 failed，分别复现空占位执行失败、用户文本混入助手进度、句柄对象不符合字符串契约。
- 修复后 OpenCode 定向测试：35 passed、0 failed。
  包含真实形状的 SSE → Session → SQLite Timeline 回归、Unicode 增量、空占位/空增量、
  最终快照去重、角色晚到、用户过滤、跨会话过滤、未知 part、资源上限、非法类型和旧句柄兼容。
- server 进程 OpenCode 测试：2 passed、0 failed。新增 Workspace 创建用例验证根 agent
  和 creation.agent 的句柄，并等待初始输入完成，检查子进程清理。
- 实际 CLI 与正式 TypeScript schema 用例见上表，单列为运行验证，不计入 Rust 测试数量或 LLVM 覆盖率。
- 完整 workspace 单元/集成测试由 LLVM 测量执行：1521 passed、0 failed、3 ignored，15 个测试目标。
  另运行 workspace 文档测试：11 个目标成功，均为 0 个 doctest。
  跳过的是既有 Claude CLI 安装探测、Claude 在线推理、Codex 在线推理测试；具体名称在共享制品中。
- 全 workspace 构建、全 targets Clippy（`-D warnings`）、Rustfmt 和 `git diff --check` 通过。

| 测量范围（LLVM summary） | 行覆盖率 | 已覆盖 / 总行数 | 相较基线（百分点） |
| ------------------------ | -------: | --------------: | -----------------: |
| Rust workspace           |   91.52% |   44143 / 48235 |              +0.04 |
| server-provider          |   92.20% |   19836 / 21513 |              +0.07 |
| OpenCode 生产插件        |   85.28% |     2474 / 2901 |              +0.79 |
| 新增原生文本 Stream      |   97.74% |       173 / 177 |       无对应旧文件 |

测量对象是上述基线加本次 Rust 修改；变更源码 SHA-256、逐文件指标、未覆盖行、
原始测量和日志指纹见 [共享覆盖率制品](opencode-streaming-coverage.json)。测量后仅更新报告与制品。
可比基线为 [前次修复覆盖率](opencode-compatibility-coverage.json)，采用同一平台、features 和命令；
小幅差异也可能包含测试执行的非确定性。
macOS arm64、rustc 1.98.1、cargo-llvm-cov 0.8.7，默认 features；工具默认过滤测试源码，
无额外文件排除，doctest 未插桩，真实 CLI 探针不计入 LLVM 覆盖率。
HTML 位于 `target/llvm-cov/html/index.html`，共享审阅使用上述 JSON。
尚未完整覆盖原生进程启动/异常关闭与部分工具/推理投影路径；新 Stream 尚缺混合父消息缓冲、
已确认用户 delta 早退及无 delta 时增长快照的部分组合。后续补充原生协议序列夹具和平台验证。

```sh
nix develop --command cargo test -p server-provider local::opencode --locked --offline
nix develop --command cargo test -p server-bin --test process opencode --locked --offline
nix develop --command cargo fmt --all --check
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo build --workspace --locked --offline
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo test --workspace --doc --locked --offline -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-stream-summary.json
nix develop --command cargo llvm-cov report --lcov --output-path /private/tmp/ait-opencode-stream.lcov
```

尚未验证：外部真实模型/账号、第三方插件、真实 2.x CLI、当前修复在 Linux/Windows 的运行、
原生 Electron 完整流程。本次真实 CLI 回归未重新执行命令允许/拒绝及取消；
这些场景的历史外部复测结果不当成本次运行结果。离线协议/进程回归保留相关覆盖。
