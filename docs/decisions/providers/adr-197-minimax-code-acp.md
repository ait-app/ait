# ADR-197：MiniMax Code 原生 ACP Provider

- 状态：Accepted。
- 日期：2026-10-28。
- 范围：MiniMax Code 注册、原生会话、控制与事件投影。
- 关系：沿用 [ADR-089](adr-089-provider-owned-composition.md) 的内置装配和
  [ADR-179](adr-179-provider-owned-tests.md) 的测试归属。

## 背景

Paseo 的 MiniMax 接入使用 `@minimax-ai/code` 的 ACP 接口。`mmx` 属于另一个产品：
MiniMax 多媒体/API CLI，不提供编码 Agent 会话接口。客户端此前保留了 MiniMax Code
目录项，但 Rust 后端没有对应 adapter。

## 决策

1. 注册身份为 `minimax-code`，调用已安装的 `mcode acp`；可通过
   `AIT_SERVER_MINIMAX_CODE_BIN` 指定程序。构造和注册不安装程序、不启动进程。
   使用当前原生程序，不再由目录项隐式下载旧版 `0.1.2`。
2. 按 Paseo 使用官方 ACP 协议；在 AIT 中通过既有内置 Provider 装配入口注册。
   本次不实现任意 `extends: acp` 配置的动态注册，也不把 `mmx` 包装成 Agent。
3. `initialize`、`session/new`、`session/resume`、`session/prompt`、`session/cancel`
   和 `session/close` 管理原生生命周期。认证、工具执行和对话上下文归 MiniMax Code。
   AIT 仅保存原生身份、工作目录和展示时间线，不重新执行历史输入来模拟恢复。
4. 模型和推理选项来自原生 `configOptions`。工作模式来自独立的 `modes`，经
   `session/set_mode` 修改；权限选项 `_permission` 单独显示为 `permissionMode` 功能，
   不把 Plan/Default 工作模式解释为工具权限。
5. 复用共享 ACP 传输与权限回调。DSH 的 ACP 消息/工具投影移至 `local::acp_streaming`，
   MCP 配置转换归 `local::acp_config`，两个 adapter 依赖共享实现，不互相依赖。
6. 使用原生流式文本、推理、工具进度及用量。图片输入保存为 AIT 可重放的图片资源。
   原生错误通过已有有界脱敏通道展示。取消必须等到原生终态；超时关闭受管进程，
   不把未知状态报告为取消成功。
7. 当前不声明原生会话导入、历史重投影、回退、实时语音或辅助摘要能力。
   已有 AIT 会话通过原生 resume 继续，展示历史仍由 AIT 持久时间线提供。

## 后果与验证

用户需要安装最新版 `@minimax-ai/code` 并执行 `mcode login`。`mmx auth login`
不能替代 MiniMax Code 登录。安装、认证与具体能力见 [操作手册](../../operations/minimax-code.md)。

离线测试在 `crates/provider/src/local/minimax/tests` 验证 native adapter 与 AIT
执行器；真实测试显式启用并只在临时工作目录执行。daemon 仅调整内置 Provider
快照与隔离测试环境。实际命令和结果见 [验证报告](../../reports/providers/minimax-code-acp-2026-10-28.md)。

**Test coverage**：未测量；用户要求完整测试流程，未要求覆盖率。若需测量，限定
MiniMax adapter、共享 ACP 投影及直接相关执行器场景，使用 `cargo llvm-cov`。
