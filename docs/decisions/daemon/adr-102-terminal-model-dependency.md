# ADR-102：Terminal 仅依赖 model 的共享契约

- 状态：接受
- 日期：2026-10-07
- 关联：[ADR-101](../providers/adr-101-provider-metadata-independence.md)

## 背景

ADR-101 已将 Project/Workspace 记录与 registry、Workspace 活动接口和连接事件资源
迁入 model。terminal 仍通过 metadata 的重导出路径访问这些定义，因此保留了一条
没有业务服务调用的同级 crate 依赖。

## 决策

1. terminal 的 Project/Workspace 记录与 registry 直接从 `model::workspace::records`
   和 `model::workspace::registry` 导入。
2. Workspace 活动状态和贡献接口直接从 `model::workspace::activity` 和
   `model::workspace::attention` 导入。terminal 继续拥有活动状态，metadata 消费投影。
3. 连接事件通道与事件类型直接使用 `model::session` 和 `model::session::protocol`。
   宿主注入原来的共享 SessionEvents 实例，通知选择、订阅和数据隔离保持一致。
4. 从 terminal Cargo 依赖中移除 metadata，测试也只使用 model 的契约。
   依赖守卫禁止普通、开发、构建、optional 和平台条件上的该依赖，同时检查测试源码。

## 后果

terminal 的唯一内部依赖是 model。此修改只切换已迁移定义的导入来源，不新增存储实现、
缓存或事件通道，也不改变终端 Workspace 放置、PTY 所有权、活动通知及关闭行为。

Workspace 业务服务仍归 metadata；终端进程、活动状态和订阅仍归 terminal。
