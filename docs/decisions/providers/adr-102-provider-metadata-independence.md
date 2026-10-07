# ADR-102：共享协作契约归 model，Provider 不依赖 metadata

- 状态：接受
- 日期：2026-10-07
- 修订：[ADR-101](adr-101-provider-summary-generator.md) 的剩余依赖边界，以及
  [ADR-089](adr-089-provider-owned-composition.md) 中 provider 的内部依赖集合。

后续 [ADR-106](../daemon/adr-106-concrete-file-persistence.md) 将具体文件实现迁到 file，
[ADR-107](../daemon/adr-107-direct-imports-from-owning-crates.md) 删除兼容转发路径，
调用处直接导入所属 crate。

## 背景

摘要生成迁移后，provider 仍依赖 metadata 的 Workspace/Project 记录与 registry、
worktree 契约、活动和关注接口、协议类型、事件通道、创建回执及文件存储。同时，
Agent 创建、导入和恢复直接持有 Directory，输入与新建 worktree 直接触发命名和 setup。

这些引用既包含真实的 Workspace 业务协作，也包含多个能力共同使用的资源。将具体
Workspace 服务全部放入 model 会混淆业务所有权；仅保留开发依赖则不能消除 crate 边界。

## 决策

1. 将共享事实和契约迁入 `model::workspace`：持久化 Project/Workspace 记录、registry
   接口、活动状态、关注接口、worktree intent/provisioning、标签存储契约和相关 wire
   schemas。保持原有序列化字段、校验、枚举值和错误语义。
2. 将共享连接事件与 presence 资源迁入 `model::session`，幂等创建回执迁入
   `model::creation`。沿用原事件 hub、订阅、身份预留、未知结果和重试语义。
3. 将原子 JSON registry、Project/Workspace 文件实现迁入 `model::storage::registry`。
   标签持久化事务与该 registry 的内部提交锁、回滚和冻结机制不可分割，因此标签文件
   存储也一起迁入 `model::storage::workspace_labels`。标签业务操作仍归 metadata。
4. `model::workspace::lifecycle` 提供 Workspace 登记、命名和 setup 协作接口。
   metadata 的 Directory、WorkspaceNames 实现接口；setup adapter 持有现有
   `Arc<Mutex<WorkspaceAutomation>>`，保留同一锁、运行时和任务所有者。
   daemon 将接口对象注入 provider；provider 不持有这些具体服务类型。
5. metadata 的原共享模块只重导出 model 定义，保留其他消费者的源码路径兼容性。
   不复制实现、缓存、订阅器或持久化状态。model 不依赖任何能力 crate。
6. 从 provider 的普通及开发依赖中移除 metadata。依赖守卫拒绝普通、开发、构建、
   optional 和平台条件上的该依赖，并检查 provider 的全部 Rust 源码，包括测试。

## 后果

provider 的内部依赖只有 domain 和 model。Workspace 的业务规则仍由 metadata 管理；
Git/worktree 的实际实现仍归 filesystem。共享摘要生成仍按 ADR-101 由 provider 提供。

创建失败清理和自动归档继续先关闭相关原生写入会话，再执行 worktree 清理。创建回执、
目录通知、Workspace 命名任务及 setup 使用原来的共享实例。磁盘文件路径和格式不变，
无需迁移用户数据。

model 现在同时包含共享契约和阻塞文件存储原语，是公共运行资源层，不是纯领域层。
domain 的依赖边界保持独立。

原有存储、协议、事件和创建测试随实现迁移。Provider 活动投影的单元测试留在 provider；
涉及 metadata WorkspaceState 的组合测试移至 daemon 组装层，不通过开发依赖恢复边界。
定向覆盖率脚本同步更新测试归属。
