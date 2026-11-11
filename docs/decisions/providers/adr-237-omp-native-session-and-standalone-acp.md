---
id: ADR-237
status: proposed
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

## 接口变化与迁移

这是结构性切换，包含明确的 CLI 破坏性变更；不能因公共 SDK 字段未变就称为“没有接口变化”。

| 边界                    | 旧接口/调用链                                                | 新接口/调用链                               | 迁移与兼容性                                                                                                               |
| ----------------------- | ------------------------------------------------------------ | ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Ait Provider 内部       | `AgentSession → bridge::Session → ACP engine → 原生 Session` | `AgentSession → session::Session → OMP RPC` | 删除内部 ACP 客户端，内部调用者改用原生 Session；不增加 UI Protocol 控件。                                                 |
| 外部程序入口            | `aitd omp-acp`                                               | `ait-omp-acp [--omp-bin path]`              | **破坏性 CLI 变更**，旧子命令已删除且没有转发别名。使用它的脚本、编辑器配置和外部 ACP launcher 必须更新可执行文件与参数。  |
| 构建与分发              | ACP 入口依附 daemon 可执行文件                               | 同版本的 `aitd` 与独立 `ait-omp-acp`        | 支持的归档与 macOS/Linux desktop 资源必须携带配对产物，分别核验版本、签名与归档；OMP 本体仍由用户安装。                    |
| 外部协议                | daemon 子命令承载标准 ACP 与协商扩展                         | 独立程序承载相同标准 ACP 与协商扩展         | 标准 ACP 客户端不必使用私有扩展；`_omp/rpc`、`_omp/event`、`_omp/respond` 仅供明确协商的外部 ACP 连接，不是 Ait 内部网关。 |
| 持久恢复                | `transport: rpc/acp`、ACP 标记句柄、`ait-acp-aliases`        | 继续读取旧身份，内部新句柄记录原生事实      | 兼容恢复只解析已有身份，不重写原生 JSONL；显式 `sessionFile` 与调用方沙箱保持权威。文件格式兼容不等于旧 CLI 兼容。         |
| UI/公共 SDK/daemon wire | 既有消息与字段                                               | 保持不变                                    | 不要求旧客户端升级；这项兼容性不覆盖上面的 CLI、内部 Rust 边界和分发变化。                                                 |

独立程序只负责 CLI/stdio 生命周期，Provider 继续拥有 ACP 转换、原生会话和 RPC。
依赖方向是独立程序调用 Provider；不得引入 Provider 到 daemon/API 的反向依赖。
沙箱问题归原生 adapter 的创建/恢复/重开边界，UI 展示问题归客户端，不能用 UI 兼容性掩盖适配器迁移。

## 后果

内部输入和事件不再经历 ACP JSON 往返，唯一原生 writer、既有 Timeline/图片/审批/子 Agent 投影和
原生结算保持不变。标准 ACP 与历史扩展客户端仍由外部 bridge 兼容；这不是 OMP 官方 TUI 或协议新能力声明。
公共 SDK/UI 消息与 daemon wire 字段未变，只说明这些客户端接口保持兼容；内部 Rust 调用链、
外部 CLI 入口以及可执行文件的分发/调用上下文按上表发生明确变化。

定向验收必须覆盖 native 创建/恢复/配置重开、别名与日志不变、沙箱失败关闭、独立 CLI 纯协议输出/EOF/错误，
以及 Linux/macOS 资源和签名归档路径。无法执行的签名或 notarization 不能用“已接线”代替通过，任务保持未完成。
当前说明见 [OMP 架构](../../architecture/omp-acp-bridge.md) 和 [操作手册](../../operations/oh-my-pi.md)。
