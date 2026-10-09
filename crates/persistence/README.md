# persistence

单文件读写、通用 registry 和具体文件持久化。依赖 model/domain 的记录与接口，不依赖功能服务。

- `File`：字节/UTF-8 读取、有界读取、同目录临时文件原子写入。
- `registry::FileRegistry<R, E>`：插入顺序缓存、同锁变更、before/after hooks、writer 注入与冻结。
  `E` 默认是通用 registry 错误，也可以由业务层实现转换。
- `storage`：Project/Workspace registry、标签事务、创建回执、daemon/project 配置、
  project icon、push token、Agent runtime 和 schedule 的具体文件适配器。
  `storage::creation::{FileReceiptStore, open}` 持久化创建回执，向 model 的创建协调器注入存储。

daemon 启动配置、故障证据文件和稳定 server identity 只由宿主使用，位于 `bins/daemon`。

读写与 registry 调用是阻塞操作，异步消费者应通过自己的阻塞任务/预算入口执行。
宿主负责跨进程写入互斥。

边界与兼容行为见 [ADR-112](../../docs/decisions/daemon/adr-112-persistence-crate.md)
和 [ADR-106](../../docs/decisions/daemon/adr-106-concrete-file-persistence.md)。
