# ADR-126：按身份读取目标 Workspace，完整目录后台同步

- 状态：已实现
- 日期：2026-10-11
- 关系：补充 [ADR-081](adr-081-directory-change-push.md) 的目录订阅路径；推进[远程打开方案](../../plans/remote-workspace-open-performance.md)的目标读取部分。

## 背景

冷启动缺少目标缓存时，Workspace 路由等待整台主机的 Project/Workspace 快照。侧栏同步和目标首屏需要不同的读取范围。

## 决策

- metadata 声明只读 `workspace.get.request`，按准确 `workspaceId` 从 registry 读取一个 Workspace 和所属 Project。返回 `ready`、`archived` 或 `missing`；不会创建、恢复或枚举完整目录。
- 活跃目标使用现有 descriptor、缓存 runtime 和活动投影。单目标活动读取保留其他 Workspace 的状态转换历史。metadata 不依赖 provider。
- Rust transport 根据已实现方法提供 `workspaceTargetRead` 能力；协议和共享 SDK 提供对应请求与响应。旧主机继续使用既有目录同步路径。
- DirectorySync 优先读取目标缓存；缓存未命中时合并同一目标的在途请求。当前连接 epoch 和本地目录修订防止迟到响应覆盖更新；冲突时重新读取。接受的目标作为增量叠加到进行中的完整快照事务。
- 部分目标只保存返回实体，不标记全局 hydrated，不保存完整目录 checkpoint，也不删除其他 Workspace。完整侧栏目录继续独立同步。
- 可见 Agent 缓存缺失时使用既有 `agent.get.request` 精确读取；没有持久化 tab 时并行请求 `agent.list.request` 的 `workspaceIds` 精确过滤和首个 40 项页面。部分摘要只做 upsert，不裁剪未下载的 Agent 或 tab，也不标记全局 Agent hydrated。工作区路由按目标结果判断缺失或触发归档恢复，按连接 epoch 重试，读取失败可显式重试。

## 后果

目标 Workspace 的响应规模与主机 Workspace 数量解耦；首屏 Agent 摘要有界，剩余目录在后台补齐。Git、终端和 timeline 仍由各自路径加载。本决策没有修改完整目录的同步协议，也没有采集 TCP、SSH 或 Relay 实网耗时。

## Test coverage

未测量：用户未请求覆盖率。相关 Rust、协议、SDK 和客户端测试分别验证目标状态、部分活动历史、目录阻塞、请求合并、重连及事件竞争；执行结果见本次交付说明。
