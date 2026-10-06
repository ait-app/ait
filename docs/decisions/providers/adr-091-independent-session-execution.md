# ADR-091：会话独立执行与 Provider 后台发现

- 状态：Accepted。
- 日期：2026-10-07。
- 修订：[ADR-088](adr-088-provider-catalog-startup-isolation.md)的串行 Catalog 与执行队列；保留其物理连接响应接纳和客户端连接就绪规则。[ADR-032](adr-032-daemon-native-provider-execution.md)的全局串行范围和阻塞存储执行位置；保留其原生会话、单 writer、持久化合并与进程租约边界。
- 参考：Paseo `0.9.0-beta.2`，提交 `2c8e8a826810337492cc5a38bb0bbd705b6fb632`，来源见[实施清单](../../plans/paseo-agent-concurrency.md)。

## 背景

ADR-088 已将 Provider discovery 从全局执行队列隔离，并允许物理连接继续读取后续消息；Catalog 内仍串行发现，native create/resume/close 仍逐条等待整个请求完成。一次 Provider discovery、native create/resume 或 close 会暂停其他会话的请求和事件处理，远程打开 Workspace 因此可能等待无关工作。

这里的会话运行实例指稳定 AIT agentId 对应的原生 session、active turn、待提交事件和输入接纳状态。它占有 native writer、子进程或传输连接、持久化句柄和有限的会话预算；名称相同、Provider 相同或 Workspace 相同都不代表同一运行实例。

## 决策

AgentExecution 保留原有可克隆 façade，内部组合短路由器、异步会话任务、独立读取路径和共享 Catalog。共享注册后的 `Arc<dyn AgentClient>`、registry、Timeline、输入/创建 receipts 和预算；每个会话任务独占自己的 AgentManager live 状态。兼容 manager 方法复用原有实现，根 manager 不持有这些 live session。

### 请求与状态所有权

| 工作 | 执行位置与提交顺序 |
| --- | --- |
| create、resume、restore、refresh、rewind、send、steer、cancel、permission、configure | 稳定身份所属会话任务；与该 session 的事件提交保序 |
| 原生事件、pending runtime/handle/input/terminal 重试、输入 FIFO | 所属会话独立 drain，只读本身份的首条待输入；沿用已有事件批量上限与 claim 先落盘规则 |
| Provider snapshot.get / refresh | 当前缓存或 loading 立即返回；refresh ack 表示接纳，后台逐 Provider 推送 |
| 模型、模式、features、available 及显式 Provider 检查 | 独立 Provider 查询预算，只等待目标发现或检查 |
| get、目录/list/sync、批量订阅身份解析、已加载 timeline | 独立读取预算；用 durable 记录和已提交运行状态投影 |
| 冷 timeline | 目标会话内去重；只读历史，不自动获得 writer；提交前核对身份、句柄、归档状态和 Timeline epoch |
| finish.wait | 每身份的 watch；监听 deadline 和停机，不反复提交命令 |
| archive、delete、items.close、Workspace 退役、自动归档 | 相关会话集合的屏障；无关会话继续执行 |

按 agentId 和 Provider native session ID 维护同一所有者的别名，在 durable 注册可见之前绑定。输入、生命周期和事件共用一个可变 session 所有者，Rust 不复制 Paseo 的多条 Promise tail；所有修改同一 writer 的入口仍保序。名称或前缀在入队前规范化，后续重命名不能改变请求目标。

保留一个专用 Tokio current-thread runtime 驱动传输 reader 和异步任务。每次会话操作、事件提交和存储读取移入 `spawn_blocking`，通过其 Handle 驱动 native async I/O。空闲会话没有专属 OS 线程；无 native 资源的任务降低轮询频率，根调度器只查待输入身份而不解码 prompt；阻塞存储不占 runtime reactor，运行中的 native I/O 不持有全局 manager 锁。

### 级联与关闭

路由器按稳定顺序向所有相关会话插入屏障，等待之前接纳的操作完成，然后执行元数据修改。即使元数据部分成功，也要求每个所有者 reconcile 已归档或删除的记录。只有相关 writer 关闭后才确认成功；关闭失败保留状态供后续重试，不重新插入删除的记录。

屏障期间新建的相关任务暂缓启动。创建提前声明 Workspace/父身份归属，并在 factory 返回后复核 Workspace、Project 和父身份；迟发现的退役 scope 拒绝 native launch。自动归档由所属会话提交到调度器，复用相同屏障；相关任务在关闭确认后继续停留在屏障内，直到 worktree 清理结束，防止 restore 在清理途中重新打开 writer。失败的自动关闭或清理保留待重试动作。

### Catalog 与客户端

按 Provider 和规范化目录合并正在执行的 discovery，每 Provider 并发 4，总并发 16，待发现任务上限 64；重复刷新只记录一个后续刷新标记。条目有效期 60 秒、单次发现含预算等待的超时 30 秒、缓存 scope 上限 16。淘汰和停机取消任务，generation 校验拒绝旧结果。

快照和 refresh ack 增加可选 `generation`、`revision`，快照增加可选 `refreshing`。内容 hash 仍只描述条目内容。旧客户端能继续解码；新客户端在目标 Provider 的 refreshing 清空或失败终态到达后结束刷新，并拒绝同一 generation 的旧 revision 覆盖新快照。现有推送、内容引用缓存和断线 refetch 保持使用。

### 预算、观察与停机

- 接纳命令全局 64，permit 保留到处理结束；Catalog 响应额外保留 64 个 transport response slot，直到响应发送或调用方取消；每会话队列 64，总任务路由项最多 128，空闲且没有 native 资源的路由项 30 秒后回收。
- 会话全局 32，create/resume 在 native launch 之前取得 permit；失败关闭仍保留 permit。历史读取 8，直接读取 16，显式 Provider 查询 16；snapshot 读取不等待查询预算。
- 传输层原有 wait 接纳预算保留。watch 发布发生在 durable 提交后，在途 admission、关闭和自动清理期间保持 busy；wait 的 permission/error/timeout 与归档完成文本继续兼容。
- 停机先拒绝新命令并唤醒 wait，drain 已接纳请求并关闭各会话，再取消剩余 Catalog 后台发现和关闭生成任务，最后终止 runtime 并释放数据目录租约。

## 后果与验证

一个 Provider 或会话的慢操作不再阻塞所有其他会话。同一会话的 writer 修改仍会等待其在途工作；共享 registry/SQLite 的短事务也仍受存储锁限制。

没有更改纯 domain 的依赖方向，没有新增数据库或公共 RPC 方法；新增批量身份解析仅是 provider 内部入口。新增 debug tracing 区分请求排队、生命周期/前台/历史/读取执行、native factory/history、事件提交、Catalog 预算等待与在途数量，不记录 prompt、环境变量或凭据。

行为测试与构建限制见[实施验证报告](../../reports/providers/paseo-concurrency-validation.md)。远程设备端到端打开耗时仍需在实际连接上采样，不能由受控并发测试推算具体提速倍数。
