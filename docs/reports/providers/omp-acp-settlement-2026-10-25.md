# OMP 必需 ACP 与原有功能迁移验证（2026-10-25）

## 改动与边界

基于 Ait `bd5e5aa571227a9004844743225a89befe324ea0` 的本次修订，OMP 的发现、
新建、草稿探测、命令和恢复统一使用 ACP。删除 Protocol 控件及 RPC 执行实现；
旧 `transport: rpc` 配置只能兼容迁移，不能选择 RPC。旧句柄继续同一份原生历史和 session ID，
保留用户消息回执，不通过重发输入完成迁移。

OMP 原生实现基于 `18.12.6` / `45c35e40c46166dc726ccc52f5d27f40f2449f99`，
完整补丁和测试归 [third-party/oh-my-pi](../../../third-party/oh-my-pi/README.md)。
Ait 本身仍使用 Rust，没有增加 domain、公共 ports 或客户端 wire 边界。
决策见 [ADR-190](../../decisions/providers/adr-190-omp-acp-settlement.md)。

## 功能核对

| 既有能力                     | ACP 路径与验证                                                                                              |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------- |
| 模型、推理和目录发现         | ACP config options 保留原生模型 metadata；保持原有 opaque ID 和每个模型的推理选项，最多 8192 模型           |
| 四个权限档位                 | 原生 Full / Write / Ask / Ask and Remember 映射保留，ACP 动态配置与 unattended 语义一致                     |
| 系统提示                     | ACP 重开原会话，保留历史与身份；不制造额外输入                                                              |
| 自动压缩/重试、fast/slow     | 标准 ACP boolean options；只读确认当前模型支持，原生选择被应用和回读，不隐藏已有覆盖值                      |
| 本机 MCP 与扩展              | 正式会话显式协商 native ownership，SDK 加载本机配置；空 ACP server 列表不清空工具，普通客户端仍隔离本机 MCP |
| 文本、推理、工具、用量和费用 | `_omp/event` 复用既有 OMP 投影，原生 assistant usage 保留 input/output/cache/cost 和 context window         |
| 原生子 Agent                 | 会话专属 EventBus 转发生命周期、进度及消息；关闭后取消订阅，保留各自 Timeline                               |
| 图片和富输入                 | ACP prompt blocks 与既有投影；原生 blob 引用按严格 hash、文件类型与 32 MiB 上限读取为 Ait 图片工件          |
| 审批与问答                   | 原生 ACP permission options 和 form elicitation，保留长期授权选项、含逗号的多选、拒绝与取消校验             |
| 取消、追加输入和后台工作     | ACP cancel / steering 与版本化 settled；审批 callback 不被控制响应吞掉，迟到工具进度不修改已完成项          |
| 持久历史、列表、导入、恢复   | 只读原生 JSONL；默认、显式旧 RPC 配置与旧句柄均通过 ACP，回执和原生身份保持                                 |
| 有界大输出                   | OMP 输出帧上限 64 MiB；其他 ACP Provider 保留原预算，9 MiB 夹具验证不会被共享的 8 MiB 默认值截断            |

RPC 原先未提供的会话/文件回退、单独控制子 Agent、每会话 MCP/工具策略覆盖与输出 schema
仍不宣称支持。所有输入在提交前验证，缺少必需 ACP 扩展时拒绝，不回退 RPC。

## 结算与错误语义

- 同时收到本轮 prompt 成功响应及匹配 session/turn 的 `_session/settled` 才完成。
  原生端等待 owner jobs/deliveries，并在同一 ACP 订阅中处理后台续跑；最终续跑决定结果。
- 过期 settled 不释放新回合；不以延时、安静窗口或超时合成成功，不自动重发已提交输入。
- 原生 `_meta.error` 与消息错误进入有界脱敏的失败路径，失败原因在关闭后仍可读取和持久化。
- Host 终态前只读原生历史，原子校正 Timeline 与用户回执。已提交历史丢失时明确失败；
  未提交空会话允许重建，草稿探测不写会话、不加载本机 MCP、不修改原生服务档位设置。

## 验证

最终 Rust 验证使用仓库固定的 Rust **1.98.1**。仅运行受影响模块及共享辅助模块消费者：

```sh
cargo test --locked -p provider local::omp --lib
cargo test --locked -p provider local::acp_stream --lib
cargo test --locked -p provider local::opencode --lib
cargo test --locked -p provider local::deepseek_harness --lib
cargo test --locked -p provider local::diagnostics --lib
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings
cargo build --locked -p provider
npm run check:docs
```

OMP 28 passed / 3 ignored；共享 ACP stream 4 passed；OpenCode 99 passed / 28 ignored；
DSH 87 passed / 7 ignored；诊断 20 passed。合计 **238 passed、38 ignored、0 failed**。
忽略项需要显式 CLI、外部环境或付费推理；没有执行完整 Rust workspace 测试。

在隔离配置下，额外指定打补丁的真实 OMP 源码 launcher 执行两项已忽略的无推理测试，
**2 passed、0 failed**。覆盖实际 ACP 初始化、新建、关闭、空会话恢复、动态模型/推理和服务档位发现，
并检查原生配置不被探测修改、无会话 JSONL。没有替换系统 CLI 或重启 daemon。

```sh
AIT_TEST_OMP_BIN=/path/to/patched-omp cargo test --locked -p provider local::omp::tests::installed --lib -- --ignored
```

原生 OMP 定向测试使用 mock 模型或本地协议服务器，不调用付费模型：

```sh
bun test packages/coding-agent/test/acp-agent.test.ts packages/coding-agent/test/acp-initialize-conformance.test.ts packages/coding-agent/test/acp-mcp-isolation.test.ts packages/coding-agent/test/agent-session-async-delivery.test.ts
bun run --cwd packages/coding-agent check:types
```

结果 **135 passed、0 failed**；修改的原生源码和测试通过 oxfmt/oxlint。
补丁通过目标 revision 的 `git apply --check --cached`。
原生 MCP 的验证范围为 factory 显式所有权、工具不被空列表清除、拒绝混合所有权及普通客户端隔离；
没有连接用户的真实远程 MCP 服务。付费模型与用户实际账号的端到端推理未运行。

## Test coverage

**Not measured**：用户未要求行覆盖率；通过项数不代表覆盖率百分比。
本次未生成覆盖率工件。若后续要求测量，使用 `cargo llvm-cov` 限定上述受影响模块并提供报告，
明确排除真实账号、付费模型和不可用平台，不扩大到完整 workspace。
