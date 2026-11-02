# 远程 Workspace 打开性能：剩余工作

更新：2026-11-03。此页保留未完成工作；改造前调用链、估算和旧 Relay 分析见
[原方案](../archive/plans/remote-workspace-open-performance.md)。原方案没有实网性能采样。

## 已有实现

- Provider 发现、只读查询与按 Agent 保序的执行已隔离，见
  [ADR-091](../decisions/providers/adr-091-independent-session-execution.md)。
- 目标 Workspace 读取与缓存/事件衔接见
  [ADR-126](../decisions/workspace/adr-126-workspace-target-read.md)。
- 持久化展示索引和页级读取见
  [ADR-125](../decisions/providers/adr-125-durable-timeline-projection-pages.md)；Codex 增量历史投影见
  [ADR-192](../decisions/providers/adr-192-codex-incremental-history-projection.md)。
- 在线业务已统一到 Hub；新的采样不再测旧 Relay 配对或 Yamux 回退，见
  [ADR-204](../decisions/clients/adr-204-hub-only-online-business-transport.md)。

## 待完成

- [ ] 在实际 TCP、SSH、Hub 连接上采集打开瀑布，分别记录首次连接、在线切换、恢复和冷缓存。
- [ ] 复核已有持久化历史先展示、原生历史后台校验的剩余阻塞；区分首次导入与重复打开。
- [ ] 根据实测决定模型目录预取、标题/Git 状态渐进呈现和目录增量分页的优先级。
- [ ] 将目录规模、每页读取/投影量与响应字节纳入测量，避免只优化 UI 后误判服务端成本。
- [ ] 补充 Linux daemon 与 Windows 远端客户端的相关验收；Windows 不构建或运行内置 daemon。

已完成并发改造清单里的远程采样及平台检查合并到这里，原记录见
[Paseo 并发改造](../archive/plans/paseo-agent-concurrency.md#实施结果与待验收项)。

## 测量与验收

使用 release/profile build，开发模式单列；未采集真实远端数据时不宣称完成实网验收。

| 维度   | 核心场景                                                |
| ------ | ------------------------------------------------------- |
| 连接   | TCP、SSH、Hub；首次连接、已在线、断线恢复、原生端回前台 |
| 缓存   | 内存、持久化、首次导入；daemon 重启前后                 |
| 规模   | 10 / 200 / 1000 Workspace；100 / 1 万 / 10 万历史事件   |
| 网络   | RTT 20 / 100 / 200 ms；5 / 20 Mbps；丢包另列            |
| 并发   | 其他 Agent 创建/运行、慢 discovery、终端和文件传输      |
| 客户端 | Desktop、Web、Android、iOS，按实际支持的连接方式        |

每个核心场景至少 30 次，报告 p50/p95、失败率、字节数、读取/投影规模和主线程长任务。
记录源码提交、平台、原生 Provider 版本和缓存条件；不能从 30 次样本稳定推断 p99。

原方案的建议目标仍需用基线校准：已在线且内存命中时，桌面框架 p95 ≤ 200 ms、可读缓存内容
p95 ≤ 500 ms；RTT 100 ms、5 Mbps 且历史已持久化时，从协议 ready 到内容显示 p95 ≤ 1 秒。
这些是验收目标，不是当前实测结果。首次原生导入与 Hub 建连单列，空白等待、失败率和内容一致性不得退化。
