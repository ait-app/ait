# Ait daemon MCP 实现与验证

## 实现范围

- Rust `/mcp/agents`：无状态 Streamable HTTP、初始化、通知、Ping、工具发现和调用。
- 40 个工具复用已有 Rust 能力分发、Workspace 生命周期、创建幂等性、输入准入、权限和 scheduler。
- Bearer、Host、Origin 校验、有界输入/输出/任务、动态 `mcp.enabled`。
- Ait 的有界列表、固定 Workspace Schedule 和持久 Heartbeat；Provider 自动注入、Browser/Voice
  session 工具不在本次范围。

参考 Paseo `9551b2cc11328170416b3d9753c7ec9a10ed35e7`。设计见
[ADR-148](../../decisions/daemon/adr-148-daemon-mcp.md)，用法见 [MCP 手册](../../operations/mcp.md)。

## 测试执行

首次验证基于 `9734c9099432a2b6f05f162376268e54725b7bff`。
PR 准备时同步 `main` 的 `1acb7f839fee856afaa7f9e95a5ea90dd56ad1ee`，
在代码修订 `6840431ed867965465391f817315de7af3c9bd9b` 重新完成以下测试、编译和 lint 检查。
仅执行受影响及直接相关测试，没有执行全 workspace 测试。

- `CARGO_INCREMENTAL=0 cargo test --locked -p api mcp --lib`：12 项 MCP Schema、参数映射、协议、错误结果和 HTTP 鉴权测试。
- `CARGO_INCREMENTAL=0 cargo test --locked -p daemon --test process unix::mcp`：3 组真实 daemon 测试，离线 Codex fixture，
  验证 Workspace 生命周期与幂等性、开关、Agent 执行/父子关系、Prompt、等待/取消、配置/元数据、
  权限查询/应答、终端、脚本、Schedule 和 Heartbeat。
- `CARGO_INCREMENTAL=0 cargo test --locked -p api http_authentication_origins_and_readiness --lib`：直接相关的既有 HTTP 鉴权回归测试。
- 测试结果：12 项 MCP API 测试、3 组 daemon 测试及 1 项 HTTP 回归测试全部通过。
- `CARGO_INCREMENTAL=0 cargo build --locked --workspace`：根 Cargo workspace 编译通过。
- `CARGO_INCREMENTAL=0 cargo clippy --locked --workspace --all-targets -- -D warnings`：无警告。
- `cargo fmt --all --check`，改动 JSON/Markdown 的 `oxfmt --check`，`git diff --check`。
- 仓库现有官方 `@modelcontextprotocol/sdk` 1.29.0 的 `Client` 和 `StreamableHTTPClientTransport`
  连接临时 daemon：握手成功，发现 40 个工具，`list_workspaces` 返回结构化结果，关闭成功。
  临时目录与进程已清理。

HTTP/进程测试使用允许本机随机端口监听的执行环境；没有外部模型调用。

## Test coverage

**Not measured**：用户未要求覆盖率测量。测试通过数不作为覆盖率百分比。
如后续要求，使用 `cargo llvm-cov` 对 `api` 的 MCP 测试和 `daemon` 的 `unix::mcp` 作限定范围测量，
并共享生成的报告。本次未验证真实外部 Provider、移动端连接和 Browser/Voice session 工具。
