# ADR-179：Provider 自身行为的测试归 provider crate

- 状态：Accepted。
- 日期：2026-10-21。
- 范围：Provider 测试归属与原生进程夹具。
- 关系：细化 [ADR-089](adr-089-provider-owned-composition.md) 的内置 adapter 所有权，
  沿用 [ADR-107](../daemon/adr-107-direct-imports-from-owning-crates.md) 的测试依赖边界。

## 背景

OpenCode、Claude 和 DeepSeek Harness 的发现、执行、原生历史与恢复场景挂在 daemon
进程测试下。验证这些 provider 行为需要启动完整宿主，测试文件归属也与实现分离。
provider 已有独立执行器和离线原生协议夹具，可以自行验证这些场景。

## 决策

1. Provider 协议、发现、执行、权限、原生历史、恢复和回退等自身行为的测试归
   `crates/provider`，按 adapter 或服务放在所属模块的独立测试子文件中。
2. 需要验证执行器、持久 registry 和 timeline 协作的场景直接使用 `AgentExecution`。
   复用 crate 内测试 helper 注入原生客户端及临时文件适配器，重建执行器验证恢复，
   并保留原生子进程退出检查。测试不依赖 daemon 可执行文件，也不增加向宿主的反向依赖。
3. daemon 保留宿主生命周期、WebSocket 传输、连接重建和跨能力组合测试。
   用例使用某个具体 provider 夹具不决定归属；断言所验证的能力边界决定归属。
   OpenCode 的 Workspace 创建响应回归因此归入 daemon 的 Workspace 创建测试组。
4. 后续新增或调整 Provider 测试遵循同一归属规则。原生程序夹具不访问真实模型服务和认证，
   可选的已安装原生程序测试仍须显式启用并使用隔离配置。

## 后果与验证

OpenCode 的导入和执行恢复、Claude 的执行恢复及文件/对话回退、DSH ACP 的权限与历史恢复，以及 OpenCode 原生进程退出后的继续
可以通过 `cargo test -p provider --lib tests::execution` 独立运行。共享执行器 helper
仍由既有执行器测试使用，生产依赖、公共接口和运行行为不变。

**Test coverage**：未测量；本次仅迁移测试，用户未请求覆盖率。如需测量，限定这些 adapter 与执行器场景。

定向验证还包括 `service::agent_execution::tests`、daemon 的 OpenCode Workspace 创建回归、
依赖边界守卫，以及格式、Clippy 和文档链接检查。
