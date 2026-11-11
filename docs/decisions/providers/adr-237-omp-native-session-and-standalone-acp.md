---
id: ADR-237
status: accepted
date: 2026-11-11
amends:
  - providers/adr-190-omp-acp-settlement.md
  - workspace/adr-223-linux-worktree-sandbox.md
paths:
  - crates/provider
  - bins/daemon
  - bins/omp-acp
  - apps/desktop
  - scripts
---

# ADR-237：OMP 原生 Session 与独立 ACP 可执行文件

## 背景

ADR-190 把 Ait 内部会话和外部 ACP 客户端放在同一个 ACP 请求引擎后。
内部调用已经持有原生 `AgentSession` 契约，再序列化 ACP 请求和 Ait 私有事件没有额外的协议边界价值，
还会把 `AgentSessionSpec` 的沙箱约束丢失在 ACP 参数重建中。
原生 Session 的权限模式/系统提示重开也不能把原有 confinement 改成 `None`。

外部 ACP 仍需独立的协议适配，但启动该协议不应要求 daemon 命令树或后台服务身份。
本决策修订 ADR-190 第 1、6 项的内部入口和第 8 项的恢复共享职责；其余原生所有权、事件映射、
结算和扩展协商规则继续生效。对 ADR-223 的补充是同一已解析的 Workspace confinement 必须穿过
OMP 创建、恢复、配置重开及会话作用域能力探测，不得按 cwd 猜测或失败后无沙箱重试。

## 决策

1. Ait 的 OMP adapter 直接创建/恢复原生 `session::Session`，通过 Rust `AgentSession` 端口调用 RPC。
   删除内部 `bridge::Session` ACP 客户端。ACP engine/protocol/mapping 只服务外部连接，仍归 provider。
   `_omp/rpc`、`_omp/event`、`_omp/respond` 仅是外部 ACP 协商扩展，不成为内部通用能力网关。
2. 新 `bins/omp-acp` 产出 Rust 可执行文件 `ait-omp-acp`。它只拥有 CLI、stdio、stderr 日志和退出码，
   调用 provider 的流式 ACP 服务；不拥有协议转换、原生 RPC、会话或模型业务。
   删除 daemon 的 `omp-acp` 子命令，不保留转发别名。正常服务时 stdout 仅输出 JSONL；
   显式 `--help`/`--version` 是独立 CLI 操作，不启动协议或 OMP。
3. OMP 始终使用用户安装的程序，按 `--omp-bin`、`AIT_SERVER_OMP_BIN`、PATH 中的 `omp` 选择。
   Ait 不下载或打包 OMP、其 SDK、Bun 或用户原生配置。桥接程序、daemon、desktop 使用同一构建版本源。
4. macOS/Linux desktop 资源与支持的独立 daemon 归档携带 `ait-omp-acp`；构建、版本核验、签名、
   notarization 和归档资源校验把它作为真实可执行文件处理。保留历史 tag 的现有重建机制，但仅在
   历史源码明确没有桥接目标时走旧布局；当前源码缺少桥接产物必须失败，不能降级发布。
   不改变 Windows desktop 不附带本地 host 的政策；不因本决策执行正式发布。
5. 原生 Session 保存从 spec 接收的精确 `WriteConfinement`，用于配置重开与会话作用域无会话探测。
   恢复沿用调用方的已解析策略，不能从历史配置或 cwd 取消沙箱。约束无法解析时失败关闭。
6. 旧 `transport: rpc/acp` 配置和带 ACP 标记的持久句柄继续可读；新内部句柄直接记录原生事实。
   `ait-acp-aliases` 文件布局、大小/类型/cwd 校验不变。别名解析移到 OMP 共享持久化模块，供原生恢复、
   只读历史和外部 ACP 使用；已有明确 `sessionFile` 的原生句柄保持权威，别名只补充缺失文件身份。
   不改写原生 JSONL，不重放用户输入，不根据别名配置覆盖调用方配置或沙箱。

## 后果

内部输入和事件不再经历 ACP JSON 往返，唯一原生 writer、既有 Timeline/图片/审批/子 Agent 投影和
原生结算保持不变。标准 ACP 与历史扩展客户端仍由外部 bridge 兼容；这不是 OMP 官方 TUI 或协议新能力声明。
没有新的公共 SDK/UI 消息或客户端必须升级的 wire 字段；旧客户端仍使用原有 daemon 协议。

定向验收必须覆盖 native 创建/恢复/配置重开、别名与日志不变、沙箱失败关闭、独立 CLI 纯协议输出/EOF/错误，
以及 Linux/macOS 资源和签名归档路径。无法执行的签名或 notarization 不能用“已接线”代替通过，任务保持未完成。
当前说明见 [OMP 架构](../../architecture/omp-acp-bridge.md) 和 [操作手册](../../operations/oh-my-pi.md)。
