# MCP 自动注入：Paseo 与 Ait 对照

- 日期：2026-10-28。
- Paseo：本地干净 checkout `91154ba4e8ace3484595a05a4366279fb4818745`；以下上游链接固定到该修订，结论来自本地源码及测试断言阅读，不代表已验证最新版。
- Ait：`22b1aae75a51eff94b3afcc843c83cd1d3153fca` 加当前工作树，包含此前已完成的 Skill 自动分发改动。本次仅新增此调查文档，没有修改 Rust。

## 结论

本文保留实现前的审计基线；后续 HTTP 运行时注入见 [ADR-202](../../decisions/providers/adr-202-agent-mcp-injection.md)。

MCP 服务端与 Skill 分发已经具备，缺的是从 daemon 到 Provider 的运行时注入与调用链。
Paseo 同时支持原生工具目录注册与 HTTP MCP；完整对齐不能仅给所有 Provider 增加一个
`mcpServers` 项。第一阶段可以先完成 Codex、Claude 和 DSH ACP 的 HTTP 路径，其他 Provider
按能力补适配。

## Paseo 的实际实现

1. **宿主提供连接上下文与开关。** HTTP listener 绑定后，使用实际地址构造 `/mcp/agents`，
   将 URL 交给 AgentManager。`mcp.enabled`、`mcp.injectIntoAgents` 的变化更新 manager 的
   注入开关和工具目录。相关 setter 只修改运行时状态，这段代码没有强制重建所有已有会话。
   来源：[bootstrap](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/bootstrap.ts#L1634)、
   [setters](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L868)。
2. **专用 MCP 凭证。** 每次 daemon 启动生成一个 `randomUUID()` capability token，作为 HTTP
   MCP 配置的 Bearer 头注入。它是每次 daemon 运行共享的凭证，不是每个 Agent 的独立令牌。
   MCP 路由自行验证；配置了 daemon 密码时也兼容该密码的 Bearer，没有密码时上游允许匿名访问。
   来源：[生成](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/bootstrap.ts#L661)、
   [路由鉴权](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/bootstrap.ts#L1512)、
   [实际鉴权函数](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/auth.ts#L128)。
3. **持久配置与启动配置分开。** `prepareSessionConfig` 产生 `storedConfig`、`launchConfig`
   和 Provider 工具策略。内置项使用 `paseo` 名称；先剥离旧的内部项，再按当前 URL、令牌和
   Agent ID 生成。其余用户 MCP 配置保留；同名但不是内部端点的用户配置也保留。
   HTTP 调用者身份使用 URL 的 `callerAgentId` 参数。
   来源：[runtime-mcp-config](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/runtime-mcp-config.ts#L1)、
   [prepareSessionConfig](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L5297)。
4. **原生工具优先，HTTP 备用。** `buildLaunchContext` 在客户端声明
   `supportsNativePaseoTools` 且策略允许时构造带调用者上下文的工具目录。
   `resolveProviderLaunchConfig` 随后删除内部 HTTP 项，避免重复提供同一组工具。
   OpenCode bridge 和 OMP 有这条原生能力；它不是所有 Provider 的统一行为。
   来源：[launch context](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L5336)、
   [OpenCode](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/providers/opencode/v2/agent.ts#L59)、
   [OMP](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/providers/omp/agent.ts#L483)。
5. **统一覆盖会话生命周期。** 创建、从持久化恢复、导入和 reload 都经过上述准备与 launch
   context，然后只向 Provider 传启动配置，向注册与存储传持久配置。不会把旧端口当成恢复依据。
   Provider 策略还能关闭编排工具或禁用其中的工具。
   来源：[create](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L1293)、
   [resume](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L1408)、
   [import](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L1474)、
   [reload](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/agent-manager.ts#L1571)、
   [工具策略](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/server/src/server/agent/paseo-tool-policy.ts#L1)。

## Ait 的具体缺项

| 范围             | 当前代码                                                                           | 要补的接线                                                                              |
| ---------------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| 宿主运行时上下文 | daemon 知道实际监听地址、主 token；Provider 没有接收内置 MCP 上下文的接口          | 由 host 注入当前端点、私有凭证、配置读取或更新源；调用时绑定 Ait Agent ID               |
| 双配置           | `AgentSessionSpec.config` 是 `StoredAgentConfig`；create 直接将其写入 Agent record | 保留持久用户配置，另生成有效运行时配置；仅运行时合并 `ait` 项，处理同名用户项和旧内部项 |
| 生命周期         | create、resume、restore、原生历史导入及后续输入分别使用配置                        | 共用配置准备流程；恢复重新取得端口、凭证和身份；后续输入不能丢掉启动时的注入            |
| 身份与认证       | MCP 已读取 `X-Ait-Agent-Id`；HTTP guard 验证 daemon 主 token，拒绝 query           | 保留现有身份请求头；若对齐 Paseo 的凭证隔离，增加只被 MCP 路由接受的专用 token          |
| 配置生效         | `injectIntoAgents` 默认 false，字段已存储和校验；没有 Provider 侧消费              | 将两个全局开关接入配置准备与工具选择，定义已有会话的刷新边界；不能只控制服务端路由      |
| Provider 能力    | HTTP 配置映射只覆盖一部分适配器                                                    | 按 Provider 选择 HTTP、原生目录或明确的未支持路径，避免注入使原有 Agent 创建失败        |
| 验证             | 已有 MCP 服务端进程测试，但未覆盖 daemon 自动填充配置                              | 增加原生启动参数捕获、自动调用、恢复、第二轮输入、配置切换及不落盘验证                  |

证据：[host](../../../bins/daemon/src/host.rs#L85)、
[会话配置契约](../../../crates/provider/src/ports/agent_session.rs#L31)、
[创建与持久化](../../../crates/provider/src/service/agent_manager.rs#L487)、
[resume](../../../crates/provider/src/service/agent_manager.rs#L607)、
[restore](../../../crates/provider/src/service/agent_manager/resume.rs#L31)、
[后续输入](../../../crates/provider/src/service/agent_manager.rs#L785)、
[鉴权及 query 拒绝](../../../crates/api/src/lib.rs#L452)、
[身份头](../../../crates/api/src/mcp.rs#L211)、
[配置默认值](../../../crates/persistence/src/storage/daemon_config.rs#L43)、
[现有服务端测试](../../../bins/daemon/tests/process/mcp.rs)。

尤其要处理后续输入：当前 manager 每次调用 `start_input` 都从 record 读取持久配置。
如果只在创建时增加 MCP，Codex 的配置对比可能将其视为一次 MCP 配置变更并重新打开原生进程；
DSH ACP 对启动配置和后续 MCP 配置不一致会直接拒绝输入。这里是根据调用代码得出的实现风险，
不是本次复现的故障。来源：[Codex](../../../crates/provider/src/local/codex.rs#L598)、
[DSH ACP](../../../crates/provider/src/local/deepseek_harness/session.rs#L105)。

### Ait Provider 现状

| Provider                    | 当前适配器的 MCP 能力                                    | 自动注入的后续工作                                     |
| --------------------------- | -------------------------------------------------------- | ------------------------------------------------------ |
| Codex                       | HTTP `url`、`http_headers` 转换已存在                    | 接入有效运行时配置，并在后续输入、配置重载和恢复时保留 |
| Claude                      | `--mcp-config` 转换已存在                                | 注入会话私有配置并覆盖恢复与重建路径                   |
| DSH ACP                     | `session/new`、`session/resume` 传递 MCP 与 HTTP headers | 接入运行时配置，并保持前后轮配置一致                   |
| OpenCode ACP                | 当前拒绝 `mcp_servers`，新建会话传空 MCP 列表            | 补 ACP MCP 能力或原生工具适配；不能统一强塞配置        |
| DSH native、Pi、Antigravity | 当前拒绝非空 MCP 配置                                    | 补各自原生工具或 MCP 适配；仅分发 Skill 不会提供工具   |

证据：[Codex/Claude 转换](../../../crates/provider/src/local/configuration.rs#L138)、
[DSH ACP 转换](../../../crates/provider/src/local/deepseek_harness/config.rs#L61)、
[OpenCode](../../../crates/provider/src/local/opencode/config.rs#L38)、
[DSH native](../../../crates/provider/src/local/deepseek_harness/native/config.rs#L19)、
[Pi](../../../crates/provider/src/local/pi/config.rs#L27)、
[Antigravity](../../../crates/provider/src/local/antigravity/config.rs#L26)。

## 建议实施顺序

1. 增加单一运行时配置准备入口，分开持久用户配置与有效启动/对话配置。
2. host 提供端点、开关和私有认证上下文，按 Agent ID 合并 HTTP 配置；沿用身份请求头。
3. 先验证 Codex、Claude、DSH ACP 的创建、第二轮输入、恢复及配置变化，并确认 Ait 的 Agent
   record、快照和持久化 handle 不含自动注入的凭证和内部配置。
4. 再对齐 Paseo 的 Provider 原生工具目录路径与细粒度工具策略，覆盖其余适配器。

Skill 继续负责工具使用指引。它不替代运行时接线；HTTP shell 示例也需要 daemon 另行提供
连接上下文，不能视为 MCP 已自动注入。

## Test coverage

本次未改变 Rust 行为，覆盖率不适用。只阅读本地固定修订源码及相关测试断言，没有执行 Rust
或 Paseo 测试；此前 Skill 分发的测试结果不作为本次自动注入的验证结果。文档执行格式与本地
链接检查。后续实现应运行上述路径的定向测试；需要覆盖率时再对相应模块做定向测量。
