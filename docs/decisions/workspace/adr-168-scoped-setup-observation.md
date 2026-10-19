# ADR-168：Setup 全局摘要与面板日志订阅分离

- 状态：已接受
- 日期：2026-10-20
- 关系：延续 [ADR-131](adr-131-metadata-capability-groups.md) 和连接拥有的订阅边界。

## 背景

全局 `workspace_setup_progress` 把完整、有界但可能较大的 Setup 日志推送给所有客户端。直接删除全局监听又会丢失后台失败提示和 Setup 入口。

## 决策

1. `metadata::automation` 拥有 `workspace.setup.summary.request` 和 `workspace.setup.subscribe.request`。摘要仅含状态、是否执行过命令和内存 revision；全局 `workspace_setup_status` 只在这些状态变化时发出，不携带命令、日志或错误正文。
2. 详细订阅只接收指定 Workspace 的 progress，身份由物理连接持有，计入共享订阅预算，沿用 `subscription.release.request`。订阅先暂停，读取初始快照，排入响应后才激活事件；关闭连接自动释放。
3. 原生 Setup runtime 每次写入快照时分配单调 revision。客户端拒绝早于当前快照的排队事件，防止 bootstrap 覆盖／回退；空快照明确清除旧详情。revision 仅在当前 daemon 进程内有效。
4. SDK 的 `OwnedSubscription` 管理释放和重连。SetupPanel 仅在保留面板激活、应用可见时持有详细订阅；离开或后台即释放。重连时清除旧观测代际，重新读取摘要和可见面板快照，旧代际请求不能回写。
5. 新客户端在每次握手后按协商的 `workspaceSetupObservation` 能力选择摘要事件，失败／受信阻塞仍可触发 Setup 入口。旧主机继续走旧状态读取与全局 progress；旧客户端仍能订阅完整 progress。日志不进入持久 Workspace 记录，也不按每一行日志刷新目录。
6. `domain::session` 声明新事件种类，`model::session` 提供 Workspace 过滤的有界连接观察器；automation 负责快照业务语义，API 只组合与发布事件。没有引入新的跨能力组具体实现依赖。

## 后果与验证

未打开面板的新客户端只接收轻量 Setup 状态；日志仍由现有 runtime 有界保留。后台失败提示与详细日志采用独立生命周期。验证覆盖 Workspace／物理连接隔离、订阅预算、释放、SDK 重连、旧事件顺序、空快照和过时代际请求，见[实施报告](../../reports/clients/paseo-0.12-port-2026-10-20.md)。
