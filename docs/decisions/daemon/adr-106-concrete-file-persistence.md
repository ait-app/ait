# ADR-106：具体文件持久化归 file，model 仅声明契约

- 状态：接受
- 日期：2026-10-08
- 修订：[ADR-105](adr-105-file-tools-and-startup-config.md) 中 file 的范围和依赖方向，以及
  [ADR-102](../providers/adr-102-provider-metadata-independence.md) 中具体 Workspace 文件存储的归属。

后续 [ADR-107](adr-107-direct-imports-from-owning-crates.md) 删除旧功能 crate 的兼容重导出，
调用处直接使用所属 crate；provider 和 schedule 的 file 依赖收缩到测试。

## 背景

只迁移通用 FileRegistry 会让 model 继续拥有具体文件 registry、标签 journal 和创建回执
的磁盘适配，并且依赖 file。单文件持久化的读取、格式校验、缓存与原子提交应由同一个
crate 实现；共享记录、接口和应用协调应能在没有文件适配器的情况下使用。

## 决策

1. file 依赖 model 和 domain 的共享契约，model 不依赖 file，也不通过 dev dependency
   重新引入它。file 不依赖 metadata、provider、schedule、filesystem 或 API。
2. Project/Workspace 具体 registry、提交后 observer 与标签 catalog/journal 恢复整体迁到
   `file::storage::{registry, workspace_labels}`，保持同一缓存、提交锁和失败恢复语义。
   原来的 model 存储实现路径移除；Workspace 身份分配仍是 model 的共享身份能力。
3. 创建流程协调、订阅和资源认领留在 `model::creation`，通过 `ReceiptStore` 注入持久化。
   `file::creation::FileReceiptStore` 负责 JSON 回执，`file::creation::open` 加载回执并返回
   协调器。重启不重试不确定的副作用，失败写入不认领身份或发布进度。
4. daemon 可变配置、project config/icon、push token、稳定 server identity、Agent runtime
   和 schedule 的具体文件适配器归 `file::storage`。格式、默认值、容量限制、路径、权限、
   不覆盖已有 identity 的发布方式及 project config 乐观版本检查保持兼容。
5. daemon/project/push/schedule 存储接口和 schedule 记录归 model；Agent runtime 接口
   位于 domain 中对应记录旁。功能 crate 的旧接口与适配器路径可重导出这些类型，
   但不保留第二份实现。SQLite、Provider 原生协议、进程执行和业务服务继续归原功能模块。
6. daemon 直接组装 file 的具体适配器；业务服务继续通过 port 协作。filesystem/API 的
   相关测试通过 dev dependency 使用 file，不把文件实现移回 model。
7. 移动单元测试到实际实现旁；服务与适配器的组合测试留在消费者。依赖守卫覆盖全部
   dependency kinds，并拒绝 model → file 以及 file → 功能服务/传输/数据库依赖。

## 后果

model 不再直接读写文件，也不拥有 FileRegistry 或临时文件依赖。file 统一拥有具体文件
持久化；创建、目录、推送租约和 schedule 执行业务继续通过接口使用它。启动配置和鉴权
接线沿用 ADR-105，鉴权实现及启动校验顺序保持不变。标签 journal 虽涉及多个文件，仍作为
一个完整的文件适配器保留，以免拆散它与 Workspace registry 的锁和恢复协议。
