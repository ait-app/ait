---
id: ADR-288
status: accepted
date: 2026-11-18
amends:
  - providers/adr-210-committed-runtime-read-snapshots.md
paths:
  - crates/domain
  - crates/persistence
  - crates/provider/runtime
  - bins/daemon
---

# ADR-288：Agent 状态延迟写入 `agents.json`，启动时由 Timeline 补齐

- 修订：[ADR-210](../providers/adr-210-committed-runtime-read-snapshots.md) 第 2 条在 Agent
  runtime registry 上的适用范围：读视图包含尚未落盘的状态更新。

## 背景

`agents/agents.json` 保存全部 Agent 记录，每次变更都会序列化整份文件、回读校验、fsync 文件与目录。
本机实测 823 条记录、2.1 MB，一个运行中的 Agent 就让它每秒被重写约一次，空闲时为零。频率来自
`persist_usage`（每条 assistant 消息的 `Usage` 事件）、`persist_info` 和 `persist_input`：
它们不比较新旧值，每次都触发整份文件的同步写入。并发 Agent 越多，写放大越大。

## 决策

1. `domain::agent_runtime::registry::AgentRuntimeRegistry` 新增 `update_status`，语义同 `update`，
   但允许适配器推迟持久化；默认实现立即写入，其他适配器和测试替身无需修改。
2. 只有三类可丢失或可重建的字段走 `update_status`：`runtimeInfo.extra.lastUsage`（用量快照）、
   `runtimeInfo`（会话下一轮刷新）、用户输入时间戳（`updatedAt`、`lastUserMessageAt`、
   `lastActivityAt`，可由 Timeline 重建）。创建、删除、归档、配置、标题、attention、
   `lastStatus` 和原生持久句柄仍立即写入：它们丢失后无法恢复，且每轮最多一次。
3. persistence 新增 `AgentsStatus`，作为 `FileBackedAgentRuntimeRegistry` 的存储：
   - 内存缓存全部记录与共享读视图，读取即时可见；
   - 立即写入失败时回滚缓存，与原 registry 语义一致；
   - 记录每条记录的序列化大小，任何会让文件超过 32 MiB 的变更（含延迟写入）在提交前被拒绝，
     不会让一次超限的状态更新阻塞之后的全部写入；
   - 延迟写入只标记 dirty；首次延迟写入时启动后台线程，每 60 秒写一次 dirty 内容；
     任一立即写入也会一并写出 dirty 内容；磁盘文件仍标记 `closed: true` 时，第一次延迟写入立即
     写出一次，使文件转为未关闭，此后崩溃仍会触发恢复；
   - `flush()` 立即写出；最后一个所有者 drop 时，有 dirty 内容才写；线程无法启动时退回立即写入，
     写入失败只记录日志，变更保持 dirty 等待下一次写入；
   - `close()` 写出最终文档并标记 `closed: true`，之后拒绝一切变更、停止后台线程，drop 与定时器
     不再写入。
4. 文件格式由裸数组改为紧凑 JSON 文档：`{"lastChangedAt": "<RFC 3339>", "closed": <bool>,
"agents": [...]}`。`lastChangedAt` 是最近一次内存变更的时间，随内容一起写入；`closed` 只在
   `close()` 写出的文档中为 `true`。读取兼容旧的裸数组，视为已正常关闭；下一次写入升级为新格式。
5. daemon 在组装 Agent 执行、`recover_turns` 读取 `lastUserMessageAt` 之前执行恢复。只有未正常
   关闭（`closed` 为 `false`）的文件才恢复：对每个 Agent 从 Timeline `entries` 按 seq 倒序读取，
   截止点为 `lastChangedAt` 往前 10 分钟，最多 4096 行，plugin 行与无法解析的时间戳跳过；取
   截止点之后最新的 `user_message` 时间，只把输入时间戳往后推，然后 flush。往前留出余量，是因为
   用户消息的时间戳早于其延迟写入的提交时间，期间其他 Agent 的写入会把 `lastChangedAt` 推到消息
   之后；时间戳只前移，余量只增加读取行数，不会写入错误值。恢复失败只记录警告，daemon 继续以
   文件状态启动。用量与 runtime 信息不重建。
6. daemon 关闭时，在释放数据目录实例锁之前写出状态，不依赖 drop 时机。正常完成的关闭调用
   `close()`：此时 `Api::wait_closed` 已等待 provider worker 结束并写完会话收尾，进程内重启时
   旧运行时残留的所有者也无法再覆盖新运行时的文件。关闭超时或出错时只 `flush()` 不关闭：
   provider worker 可能仍在收尾，其写入必须成功；文件保持未关闭，下次启动执行恢复。

## 后果

- 单个活跃 Agent 的 `agents.json` 写入从约每秒一次降到每分钟最多一次加每轮少量结构性写入。
- 进程被强制终止、abort 或断电时，最多丢失 60 秒的用量快照与 runtime 信息，直到下一轮刷新；
  输入时间戳由 Timeline 补齐。结构性变更不受影响。
- 变更只校验该记录的序列化往返与累计大小，不再每次复制、序列化和回读整份记录；写入改为紧凑
  JSON，文件比原先的缩进格式更小。
- 恢复可能把未丢失的输入时间戳略微后移：`persist_input` 记录 host 接纳时间，Timeline 行使用
  条目自身（可能是 Provider）的时间戳。这只发生在未正常关闭之后，不影响排序，但会改变
  `recover_turns` 由 `lastUserMessageAt` 派生的重启提示键。单个 Agent 在余量窗口内超过 4096 行时，
  扫描在截止点前停止并记录 debug 日志，该 Agent 不恢复。
- 旧版本 daemon 无法读取新格式的 `agents.json`。降级前需把文件改回 `agents` 数组本身。
- `FileBackedAgentRuntimeRegistry` 不再使用通用 `FileRegistry`；Project/Workspace registry 不变。

## 验证

persistence 覆盖延迟写入可见但未落盘、定时与 drop 写出、立即写入携带 dirty 内容、失败回滚与
重试、超限变更提前拒绝、`close()` 标记与之后的拒绝、正常关闭后第一次延迟写入重新打开文件、
未关闭文件的 `lastChangedAt` 上报，以及旧格式读取与升级；provider 覆盖 Timeline 倒序扫描的截止、余量内的消息、plugin 行与无效时间戳，
以及时间戳只向后推进。daemon 进程测试按新格式读取文件，并验证 SIGTERM 正常关闭后文件标记
`closed: true`。
