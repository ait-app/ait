# ADR-088：Provider 发现与启动历史同步隔离

- 状态：Accepted。
- 日期：2026-10-06。
- 范围：Provider command admission、Electron 本机连接恢复。
- 关系：细化 [ADR-032](adr-032-daemon-native-provider-execution.md) 的执行器资源所有权与
  [ADR-039](adr-039-agent-timeline-provider-creation.md) 的 Provider catalog 调度；协议不变。

## 背景

恢复的桌面页面会同时请求历史和模型目录。Provider discovery 可能启动原生进程并等待插件、
模型清单；把它放在 Agent execution 的串行命令队列里，会阻塞不同 WebSocket 上的 Agent
读取、timeline subscription 和已缓存历史。连接仍能响应 ping，因此在线状态不能证明历史已同步。

另一个独立竞争发生在本机连接恢复：保存的随机端口属于上一个 daemon 实例。Host registry
先于 managed daemon 启动，立即探测旧端口时，主进程尚无可注入的当前实例 token，导致 Bearer
校验错误。端点更新也可能加入旧探测的等待过程。

## 决策

1. Provider catalog 在现有专用线程的 current-thread runtime 内使用独立有界命令队列，
   容量为 64。六个 catalog 方法直接进入该队列；Agent metadata、native lifecycle 和 turn
   调度仍由原执行队列串行拥有。两条队列共享只读 `Arc<dyn AgentClient>`，不共享可变 session。
   WebSocket dispatch 在有界接纳后登记受跟踪的响应任务，以原 request ID 返回结果，不让
   慢目录请求阻塞同一物理连接的后续历史、订阅或 ping；单连接 relay 同样适用。
2. Catalog 内仍串行执行发现，保留十六个 cwd scope、六十秒缓存、内容 hash、显式 refresh
   和事件发布语义。并发请求复用首个发现后的缓存；队列满时返回 `catalog_busy`。
3. 已接纳发现不会因请求方断开而丢失。停机关闭接纳并收尾两条队列；专用 runtime 和 data-dir
   lifetime guard 保持到两路退出，不留下脱离宿主生命周期的发现任务。
4. Electron 恢复 registry 时保留 managed host 和缓存页面，但暂不探测保存的 managed endpoint。
   主进程完成鉴权 readiness 后，由现有启动服务登记当前端点，立即释放其连接。其他 remote
   host 照常连接；停用本机管理不会因恢复旧连接而隐式启动 daemon。
5. 连接配置变更使旧探测结果失效，并立即尝试新端点。旧请求晚到时关闭其临时 client，
   不能覆盖新连接。

## 后果与限制

模型目录加载不再占用历史和执行命令的队首。该隔离不改变原生发现自身的期限，也不保证
原生历史读取、会话恢复或磁盘 I/O 永不等待；这些操作仍须保留各自错误和恢复语义。

没有扩大鉴权范围、向 renderer 暴露本机 token、放宽测试超时或取消合法的历史同步状态。
确定性测试以被显式 gate 暂停的离线发现验证历史/订阅可用性，并覆盖缓存、取消、容量与停机。
