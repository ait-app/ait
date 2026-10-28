# ADR-156：初始化进度与活动事件使用 Ait 标准名称

- 状态：Accepted。
- 日期：2026-10-18。
- 范围：Rust session events、共享协议、SDK、App 与测试。
- 修订：[ADR-081](../workspace/adr-081-directory-change-push.md) 的两个事件名称，沿用
  [ADR-094](adr-094-canonical-ait-client-methods.md) 的标准命名边界。

## 背景

daemon 已生产工作区初始化进度和 Agent 执行失败活动，但两个事件仍使用下划线名称。
Rust transport 构造的 `sessionEventTypes` 遗漏了它们，App 的 `supportedOnly` 筛选因此移除
实际订阅需求，Setup 面板收不到实时进度。

## 决策

- 初始化进度统一使用 `workspace.setup.progress`，活动记录统一使用 `activity.log`。
  订阅参数、daemon outbound method、共享协议 discriminator、SDK 和 App 监听都使用同一名称。
  旧名称不作为 wire alias，payload 和事件生产职责沿用现有实现。
- metadata 安装时声明 `workspace-setup-events-v1`；provider 安装时声明
  `activity-log-events-v1`。客户端只有同时看到对应标记和所属服务方法，才将标准名称加入
  `sessionEventTypes`。旧 daemon 缺少这些标记时跳过这两项，不向旧服务发送未知名称。
- App 沿用 [ADR-168](../workspace/adr-168-scoped-setup-observation.md) 的轻量全局摘要与按面板
  订阅详情。支持 `workspaceSetupObservation` 的主机继续订阅 `workspace_setup_status`，
  详细面板通过独立订阅接收标准 `workspace.setup.progress`；缺少独立观测能力时才在全局
  请求进度。活动记录沿用现有消息消费，不新增独立日志页面或重复的 Agent 失败通知。
- 其他 session event 的名称和订阅参数映射不在本次迁移范围内。

## 后果与验证

daemon、共享协议与 SDK 需要同步重建。Setup 的状态查询仍提供初始快照，标准进度事件负责
后续更新；聊天 timeline 继续承担 Agent 失败的持久展示。

回归测试覆盖能力标记与服务方法缺失时的筛选、标准名称订阅、消息校验、订阅归属和释放。
Rust session 测试固定两项标准 outbound method，并验证对应订阅可以收到各自 payload。
真实 daemon 的 SDK 集成测试将两项加入启动订阅断言。
