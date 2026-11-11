---
id: ADR-265
status: proposed
date: 2026-11-11
amends:
  - providers/adr-239-persisted-timeline-export-downloads.md
paths:
  - crates/provider/src/local/omp
  - crates/provider/src/ports
  - crates/provider/src/service/agent_manager
  - crates/provider/src/service/agent_execution
  - crates/provider/src/rpc/agent_execution
  - crates/provider/src/dispatch
  - crates/api/src/connection
  - crates/api/src/guest
  - packages/protocol
  - packages/client
  - apps/mobile/src/components
  - apps/mobile/src/stores/download-store.ts
  - apps/mobile/src/i18n/resources
---

# ADR-265：OMP 原生 HTML 导出与一次性客户端下载

## 背景

OMP 自带公开的会话 HTML 渲染器。用户需要的是 OMP **原生格式**的导出，而不是 Ait 通用
Timeline 导出的 HTML。原生渲染保留 OMP 自己的排版与查看器，字节可能包含活动脚本和上游
查看器资源，因此只能作为**下载**交付，绝不在 Ait 内联渲染、执行或“清洗”成通用 HTML。

**实测格式风险（非假设）**：对已安装 OMP 18.17.0 的隔离探测确认，原生导出 HTML 内嵌
`<script>` 与 base64 编码的**完整原生日志**，并从 `stencil.so` / `build.stencil.so` 加载
上游查看器 JS。打开该文件时外部查看器可以读取其中包含的会话历史。因此原生导出**不是**
与通用 Ait HTML 等价的惰性/隐私安全产物：通用 Timeline HTML 仍保持转义 + CSP 惰性输出，
原生格式按用户要求保留真实字节，仅下载、绝不自动预览/打开/内联执行，不做清洗、改写或
第二格式。UI 在原生选项旁以九种语言明示该风险（`nativeHtmlExport.risk`）。

本提案属于 PR #612 拆分 task27，依赖 task01（OMP 原生 Session）、task04（只读统计入口）与
task26（已保存 Timeline 导出下载管线）。任务和实测证据见
[task27 记录](../../plans/pr612-split/task27.json)，总契约见 [Draft #647](https://github.com/ait-app/ait/pull/647)。
提案状态不代表已经合并或全部验收通过。

## 原生导出机制与写者隔离

1. 复用**已经拥有的活动 OMP 会话**的同一 RPC 传输，发送公开 `export_html` 命令
   （`outputPath` → `{path}`）。不启动第二个 OMP 进程、不 resume、不取 lease、不发送
   prompt/steer/abort/new_session。`exportToHtml` 只读取 sessionManager 与当前状态渲染日志。
2. 采用与 steer/cancel 相同的**相关化非阻塞**路径：`begin()` 写入命令，响应经 `poll_turn`
   消费，绝不占用阻塞式 `request()` 通道，因此活动轮次的事件轮询不被中断。渲染受既有
   30s 传输期限约束；超时只失败该导出，不杀主进程、不影响对话。
3. 仅支持 provider 为 `omp` 且存在活动会话的 Agent；其它 provider、无会话或观察态 Agent
   明确返回不支持。内存态（未持久化）会话被原生渲染器拒绝，同样明确失败。

## 接口变化与消费者迁移

| 边界与所有者 | 旧接口 | 新接口 | 迁移、兼容与未改变项 |
| --- | --- | --- | --- |
| Provider/API 公共 RPC | 仅通用 Timeline 导出 | `agent.native_html.export.request` 与 `agent.native_html.export.cancel.request` | 新增接口，不替换通用 Timeline/消息/原生接口；无任意 native-action 网关 |
| Protocol/SDK | 无原生导出结果或方法 | 类型化一次性 artifact receipt；`exportAgentNativeHtml`、`cancelAgentNativeHtmlExport` | `agentNativeHtmlExport` 必须同时协商两个方法；旧 Host 不发请求，旧客户端继续读取原有消息；生成请求不自动重试 |
| Provider 端口 | `AgentSession` 无原生导出 | `begin_native_html_export`/`cancel_native_html_export` 默认拒绝；`AgentTurnEvent::NativeHtmlExport`；`AgentClient::native_html_export_directory` | 仅 OMP 实现；其它适配器默认不支持，穷尽 match 已迁移 |
| Provider → API → Files | task26 的 `GeneratedArtifact` 交接 | 复用同一 `GeneratedArtifact` 与既有 Files 一次性 grant registry | 不新增第二套 token registry；API 穷尽 Completion 已迁移；grant/visitor/过期/字节边界不变 |
| 客户端 UI/下载 | 通用 Timeline 导出控件与下载管线 | OMP 专属原生 HTML 区块；共享 `startGeneratedExport`/`downloadGeneratedArtifact` 管线 | 通用 Timeline 导出保持不变；原生字节按原样下载，`browser_handoff` 语义不变；九种语言新增说明 |
| 内部事件契约 | `AgentTurnEvent` 派生 `Debug, Clone, PartialEq, Serialize, Deserialize`，无原生导出事件 | 新增 `NativeHtmlExport` 变体；派生改为 `Debug, Serialize, Deserialize`（文件句柄不可 Clone/比较，全库无整体 Clone/PartialEq 消费者）；该变体 wire 形式为**仅元数据**（ok/fileName/size 或 error），绝不传字节或服务器路径，反序列化得到失败结果；OMP bridge 的两处 `==` 比较迁移为 `matches!` | 内部所有权事件经类型化回复通道交付，不进展示历史；既有变体的 wire 序列化不变；无新增端口抽象 |

旧 Host 缺少能力时只显示不可用，不回退到通用 Timeline HTML 或其它原生命令。

## 所有权、限制与清理

- 适配器在 **OMP state 根内**、与原生 `sessions` 同级的私有目录 `ait-native-exports`
  （随 `PI_CODING_AGENT_DIR`/profile/XDG 解析）以 `create_new` 独占创建
  `ait-native-<uuid>.html`，先证明所有权再交给原生渲染器写入。该目录位于既有沙箱
  `OMP_STATE` 允许树内，受限（confined）与不受限会话都可写；**不使用** daemon data-dir
  或工作区路径，不放宽任何沙箱策略，不触碰 Ait 数据库根。
- 原生成功后校验：常规文件、canonical 路径限制在导出目录内、精确自有 basename、
  `size ≤ 16 MiB`（超限明确 `ResourceExhausted`，绝不截断）。随后把字节**原样**拷入匿名
  `tempfile`（`GeneratedArtifact`）并删除自有命名文件；此后 grant/过期/传输生命周期
  拥有该匿名文件，与 task26 完全一致。
- 失败、取消、超时、会话关闭或配置重开都只删除**本次自有**的暂存路径，绝不删除既有文件。
  客户端取消经既有 cancel RPC 释放 grant；artifact 丢失/过期/长度不符报告失败并回收，
  不重新执行导出、不自动生成新 token、不重放原生输入。
- 复用 task26 的 16 MiB `MAX_ARTIFACT_BYTES`、2 个生成槽、8 个 artifact 槽与 60s grant TTL；
  不新增抽象类，不为 task29 预留接口。

## 验证与限制

定向测试覆盖相关化响应、活动轮次不中断、日志字节不变、所有权/限制/清理、guest 下载额度与
旧 Host 降级。真实浏览器验收须在活动原生轮次中经现有可见入口保存原生 HTML，并证明没有额外
输入、写者或中断，且下载字节与 receipt 一致。实测结果与未完成项持续写入 task27 记录。
此特性不包括 DSH ZIP 导出或新的平台下载系统。