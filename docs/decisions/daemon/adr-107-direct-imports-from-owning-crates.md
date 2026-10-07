# ADR-107：共享组件直接从所属 crate 导入

- 状态：接受
- 日期：2026-10-08
- 修订：[ADR-102](../providers/adr-102-provider-metadata-independence.md) 和
  [ADR-106](adr-106-concrete-file-persistence.md) 中用于保留旧 Rust 路径的兼容重导出。

## 背景

共享记录、契约和文件实现已经迁到 model、domain 和 file，旧功能 crate 仍有仅包含
`pub use` 的转发模块。这些路径增加跳转，并让类型看起来仍由旧 crate 拥有。
调用者已经能直接依赖所属 crate，不需要继续保留迁移期间的别名。

## 决策

1. 删除 metadata 的共享 model 转发目录、文件 storage 转发目录、creation/session 服务
   转发模块，以及共享 ports/protocol 的转发模块。metadata 保留自己实现的服务、端口、
   RPC 和协议类型。
2. 删除 provider 的 Agent runtime 接口和文件实现转发模块、schedule 的文件存储转发
   模块、protocol 的 subscription 转发模块和 API 的 outbound 转发模块。调用处直接
   使用 model、domain 或 file 的公开定义位置。
3. 同步移除实现文件中迁移遗留的跨 crate 重导出，内部使用改为普通私有导入。
   crate 的公共入口及对私有实现模块的导出继续保留，例如 `Service` 和 `File`。
4. provider 和 schedule 的生产代码通过存储接口接受宿主注入，移除 file 的正常依赖；
   相关组合测试通过 dev dependency 使用真实文件适配器。依赖守卫同时限制 API、
   filesystem、provider 和 schedule 只能在测试中依赖 file。
5. 原来挂在 metadata/schedule 文件转发模块下的组合测试移到 push 服务和 schedule
   engine 旁，继续验证真实文件存储与业务服务的协作。

## 后果与验证

Rust 调用路径明确反映组件所有权，旧转发路径移除。协议字段、持久文件格式和运行行为
保持兼容。`DaemonConfigStore` 等存储契约继续归 model，具体文件实现继续归 file。
验证覆盖 workspace 全目标编译、依赖守卫、迁移的组合测试及相关消费者的定向测试，
并运行格式、lint 和文档链接检查。
