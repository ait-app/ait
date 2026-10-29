# MCP 注入复核：参考 Paseo 当前实现

- 日期：2026-10-30。
- Paseo：`1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2`，本次通过远端只读查询确认的
  `main`；本地使用该提交的 Git 对象阅读，没有切换或修改 Paseo checkout。
- Ait：`20baa5271d96ad34eef2940896d4437f28837cad` 加 `feat/agent-mcp-injection`
  未提交工作树。本报告复核首批 HTTP 注入实现，补充
  [2026-10-28 的实现前对照](paseo-mcp-injection-comparison-2026-10-28.md)。

## 已对齐的主链路

Paseo 的连接由宿主和 Provider 建立，Skill 提供工具使用说明。宿主生成本次 daemon
运行专用的 MCP capability token，监听成功后取得实际 MCP URL；配置开关影响后续会话
准备过程，相关 setter 本身不强制重建所有现有会话。来源：
[令牌生成](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/bootstrap.ts#L703)、
[监听与配置开关](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/bootstrap.ts#L1764)、
[manager setter](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L890)。

`prepareSessionConfig` 分开 `storedConfig` 与 `launchConfig`；create、resume、import、reload
都重新准备启动配置，向 Provider 传运行时配置，向注册与存储传持久用户配置。
调用者上下文进入工具目录，或在 HTTP 配置中随请求传递。来源：
[统一准备](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L5500)、
[create](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L1371)、
[resume](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L1494)、
[import](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L1563)、
[reload](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L1666)。

Ait 当前已采用这个主链路：[host](../../../bins/daemon/src/host.rs) 提供实际端点、
专用令牌和共享配置源；[AgentMcp](../../../crates/provider/src/service/agent_mcp.rs) 与
[会话准备](../../../crates/provider/src/service/agent_manager/mcp.rs) 生成仅供原生会话使用的
连接，live binding 保证后续输入沿用同一连接；
[恢复入口](../../../crates/provider/src/service/agent_manager/resume.rs) 重新取得运行时信息。
持久 record 保留用户配置，设计见 [ADR-202](../../decisions/providers/adr-202-agent-mcp-injection.md)。

## 本轮已完成的两项补全

1. **MiniMax Code 的 HTTP 能力接线。** 首批开关只声明 Codex、Claude、DSH ACP；但
   [MiniMax 配置校验](../../../crates/provider/src/local/minimax/config.rs) 已接受 HTTP MCP，
   [原生会话](../../../crates/provider/src/local/minimax/session.rs) 在 `session/new` 与
   `session/resume` 中调用共享
   [ACP MCP 转换](../../../crates/provider/src/local/acp_config.rs)，包含 HTTP headers。
   本轮已在 [MiniMaxClient](../../../crates/provider/src/local/minimax.rs) 声明
   `supports_agent_mcp = true`，复用现有 HTTP 通路，并在 provider 内验证启动、恢复和多轮
   配置一致。
2. **配置更新也使用有效配置。**
   [manager.configure](../../../crates/provider/src/service/agent_manager.rs) 的首批实现将持久
   config 传给原生 `validate_config_update`，而 MiniMax 会比较启动时的 MCP 配置。
   注入后，两者会因内部 `ait` 项不同而误拒更新。本轮已将当前 live binding 合并为有效
   配置，再传给 `validate_config_update` 与 `validate_selection`；registry 仍只向原记录
   写用户 patch。这与 Paseo 的双配置原则一致，也保持凭证不进入持久配置。

[新增测试](../../../crates/provider/src/local/minimax/tests/mcp.rs) 使用真实 MiniMax adapter
与离线 ACP fixture，检查 `session/new`、`session/resume` 的 HTTP URL、Authorization 和
`X-Ait-Agent-Id`，连续三轮输入与模型、模式更新，真实 MCP 配置变更仍被拒绝，重新打开后
取得新端口和令牌，以及实际 `agents.json` 和新 registry 快照没有内部凭证。测试不调用
真实模型或额外 MCP server，因此这些结果验证的是自动配置传递与生命周期，不能称为真实
MiniMax 客户端完成 MCP 握手联调。

测试先验证了缺能力声明时无法取得注入项；只增加声明后，配置更新失败于 `InvalidRequest`；
同时修复配置验证后通过。完整结果见下文。

## 应保留的差异

Paseo HTTP URL 使用 `callerAgentId` query；Ait 沿用 `X-Ait-Agent-Id`，并继续拒绝 query。
两者都能为编排调用携带调用者上下文，没有必要为了复制 URL 形态而改变 Ait 的已有约束。
Paseo MCP 路由接受专用 token 或 daemon 密码，未设密码时允许匿名；Ait 继续要求有效
主凭证或 MCP 专用凭证，后者只适用于 MCP 路由。来源：
[Paseo 运行时配置](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/runtime-mcp-config.ts#L29)、
[Paseo 路由认证](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/auth.ts#L128)、
[Ait guard](../../../crates/api/src/lib.rs)、[Ait 身份头](../../../crates/api/src/mcp.rs)。

Paseo 将名称为 `paseo` 且 HTTP/SSE URL pathname 为 `/mcp/agents` 的项视为旧内部项，
先删除，再按当前端口、token 和身份重建；恢复测试覆盖了旧端口与关闭注入的情况。
但该判断不检查 host，也不证明配置来源：用户同名远端 `/mcp/agents` 同样会被删除。
Ait 当前保留所有显式用户 `ait` 项，应保留这个取舍。来源：
[剥离规则](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/runtime-mcp-config.ts#L6)、
[恢复测试](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.test.ts#L3759)、
[保留自定义同名配置测试](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.test.ts#L3825)。

当前没有发现 Ait 自动注入项回流到持久 record 的来源：
[Codex 导入](../../../crates/provider/src/local/codex/native_sessions.rs) 仅还原模型、推理和模式，
[Claude 导入](../../../crates/provider/src/local/claude/history.rs) 仅还原模型；
[refresh 的 apply_history](../../../crates/provider/src/service/agent_manager/native_sessions.rs)
不替换 `record.config`；configure 和 restore 只提交用户输入的配置。
因此“旧内部项清理”目前是防御性扩展，不能报告为已复现的回流故障。未来若增加迁移或完整原生
配置回读，应同时携带可证明的内部 ownership，并在导入边界剥离；仅按名称、路径或 loopback
地址猜测来源不足以安全删除用户配置。

## 后续能力

Paseo 对声明 `supportsNativePaseoTools` 的 OpenCode bridge 与 OMP 提供带调用者上下文的
原生工具目录，然后删除重复的内部 HTTP 项；其余路径仍可使用 HTTP。它还对外部 MCP
配置检查支持能力，内部 MCP 则允许 best effort。Ait 当前仅有 HTTP 能力布尔值，尚无这条
原生目录交付通路，不能靠对所有 Provider 强塞 `mcpServers` 达成完整对齐。来源：
[构造目录与去重](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L5590)、
[OpenCode](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/providers/opencode/v2/agent.ts#L59)、
[OMP](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/providers/omp/agent.ts#L483)、
[外部 MCP 能力检查](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/agent-manager.ts#L3891)。

Paseo 另有 Provider 级 `paseoTools.enabled` 和 `disabledTools`，会话打开时保存策略，
HTTP 与原生目录使用同一过滤规则。Ait 的 `toolPolicy.preapproved` 表示原生审批授权，
不能代替目录和执行入口的启停策略。后续应让同一可调用工具目录同时支持 HTTP 与原生交付，
在相同规则下筛选工具，再逐个接入其余 Provider。来源：
[策略解析](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/paseo-tool-policy.ts#L1)、
[工具注册过滤](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/agent/tools/paseo-tools.ts#L737)、
[HTTP 调用者策略](https://github.com/getpaseo/paseo/blob/1aef1ec6625f2c32cbd62cfbeb02cffb43a275d2/packages/server/src/server/bootstrap.ts#L1610)、
[Ait 审批转换](../../../crates/provider/src/local/configuration.rs)。

## Test coverage

覆盖率未测量：本次未请求覆盖率。若要求测量，使用 `cargo llvm-cov`，范围限定为此次
MCP 准备、配置更新与 MiniMax adapter 的相关测试，不扩展为全 workspace 测试。

以下定向验证对应本文顶部所述 Ait 修订加当前未提交工作树；测试数量不代表覆盖率：

- `RUSTC_WRAPPER= cargo test --offline --quiet -p provider local::minimax::tests --lib`：
  16 passed，4 ignored；真实原生客户端测试未显式启用。
- 加强磁盘读取与新 registry 快照断言后，
  `RUSTC_WRAPPER= cargo test --offline --quiet -p provider local::minimax::tests::mcp --lib`：
  1 passed。
- `TMPDIR=/private/tmp RUSTC_WRAPPER= cargo test --offline --quiet -p provider service::agent_manager --lib`：
  105 passed。
- `RUSTC_WRAPPER= cargo test --offline --quiet -p provider service::agent_mcp --lib`：3 passed。
- `RUSTC_WRAPPER= cargo clippy --offline --quiet -p provider --all-targets -- -D warnings`、
  `cargo fmt --all --check` 与 `git diff --check`：通过。

Paseo 部分仅阅读固定修订源码及测试断言，没有执行 Paseo 测试。本次没有运行完整 Rust
workspace 测试，也没有测量覆盖率或生成覆盖率报告。
