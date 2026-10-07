# ADR-105：File 工具、通用 Registry 与启动配置独立成 crate

- 状态：接受
- 后续修订：[ADR-106](adr-106-concrete-file-persistence.md) 将具体文件适配器一并迁入 file，并反转与 model 的依赖。
- 日期：2026-10-07
- 修订：[ADR-102](../providers/adr-102-provider-metadata-independence.md) 中通用文件引擎的归属。
- 关联：[ADR-104](../workspace/adr-104-filesystem-model-collaboration.md)。

## 背景

model 的 FileRegistry 已被 Workspace、Agent 存档和创建回执共同使用。它的缓存、提交锁、
原子替换和冻结机制属于通用文件工具，但实现依赖 Workspace 的错误类型。启动 TOML
读取在 bins/daemon，daemon 可变配置又在 metadata 中单独实现原子写入。

## 决策

1. 新建基础库 `file`，不依赖任何 workspace crate。`File` 提供阻塞字节/文本读取、
   有界读取和同目录临时文件原子写入，文件与父目录在支持的平台上同步落盘。
2. `File::watch` 使用可配置周期的 Tokio 观察任务，元数据读取离开 reactor，监测创建、
   修改、原子替换、删除及读取失败。通知通过 watch channel 合并，释放 handle 取消任务。
   这是最新变化观察，不是逐次文件操作日志；多个轮询之间的修改可能合并。
3. 通用 `FileRegistry` 全部迁至 `file::registry`：加载、插入顺序、重复身份处理、暂存修改、
   同锁 before/after hooks、原子写入、缓存发布、writer 注入和冻结。独立的存储错误类型
   可转换为调用者错误，不反向依赖 model。宿主继续持有跨进程数据目录锁。
4. Project/Workspace 记录、业务 registry 接口、提交后通知和标签事务继续归 model；
   它们使用新的通用引擎并保持现有错误语义。Agent 存档与创建回执直接使用 file 引擎。
   model 和 metadata 的旧路径不再重导出通用 FileRegistry。
5. `bins/daemon/src/config.rs` 及测试迁至 `file::config`。CLI、环境变量、TOML 优先级、
   配置路径与凭据脱敏保持兼容。API 仍拥有 token 和浏览器来源的校验实现，daemon
   注入这些校验函数；token 校验仍在文件读取之前执行。file 不持有鉴权实现或 API 依赖。
6. metadata 的 daemon 可变配置仍拥有 schema、默认值、patch、缓存与 reload 分类，
   文件读取和原子写入复用 `file::File`。监听工具不隐式启动配置热重载或业务重配置。
7. 依赖守卫允许 daemon、model、metadata、provider 使用 file，并拒绝 file 向业务、
   领域或传输 crate 的依赖，覆盖 dev/build/optional/平台条件依赖。

## 后果

通用 Registry 与配置读写复用统一文件工具。当前存储格式、用户数据路径、创建回执与标签事务的共享
实例、提交锁和恢复协议保持兼容。功能服务继续拥有数据与业务规则，file 作为基础库
独立测试文件行为与启动配置，原业务测试继续验证 registry 组合和事务失败恢复。
