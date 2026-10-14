# ADR-111：纯业务数据归 domain，连接协议并入 model::server

- 状态：接受
- 日期：2026-10-08
- 修订：[ADR-038](adr-038-daemon-protocol-dependencies.md) 的独立 protocol crate；
  [ADR-102](../providers/adr-102-provider-metadata-independence.md)、
  [ADR-103](adr-103-terminal-model-dependency.md)、
  [ADR-104](../workspace/adr-104-filesystem-model-collaboration.md) 和
  [ADR-106](adr-106-concrete-file-persistence.md) 中共享数据归 model 的位置。
  沿用 [ADR-107](adr-107-direct-imports-from-owning-crates.md) 的直接导入规则。

## 背景

protocol 的生产消费者只有 API，大量内容转发 model 的 server 定义，独立 crate 增加跳转。
model 同时拥有不依赖运行时的业务数据和 Tokio 请求资源；domain 只覆盖 Agent 身份与配置，
不能反映共享 Project、Workspace 和调度数据的实际归属。

## 决策

1. 移除 Rust protocol crate，将 `Hello`、`VersionOffer`、`ClientMessage`、能力协商和
   单连接标识并入 `model::server`。响应 envelope、错误码和 ID 验证也集中到该模块，
   与 server identity、生命周期、订阅释放消息、公开预算及版本定义共用一个入口。连接协议和
   `model::methods` 的组件方法元数据属于传输与调度基础设施。
2. domain 拥有业务数据：Project/Workspace 持久记录、标签、活动状态、注册与 worktree
   意图、Git/Forge 快照、业务请求/响应 schema、身份和 descriptor 纯投影；调度记录、
   创建进度与回执、目录 checkpoint、排序/分页值与变化元数据、连接事件与 presence 数据、摘要
   输入输出，以及存储边界交换的 revision、事务结果和错误值。
3. model 保留协作和持久化端口、summary future、请求 Context、Runtime、事件与 outbound
   队列、presence 观察资源、目录序列维护、分页算法和创建协调。它依赖 domain；domain 不依赖
   model、功能 crate、Tokio、传输适配器或具体存储实现。
4. 消费者直接从 domain 导入业务值、从 model 导入端口和运行资源，不增加跨 crate
   转发别名。功能 crate 之间继续通过端口协作；file 实现存储端口，宿主装配适配器。
5. 相关单元测试和 Paseo JSON 夹具随数据定义迁移到 domain，协商测试迁到
   `model::server`。更新夹具生成器、定向覆盖率脚本和依赖守卫，拒绝 protocol 回归
   以及 domain 的所有向外依赖，包括 dev/build/optional/平台条件依赖。

## 后果与验证

Rust workspace 减少一个 crate。纯业务值可以独立于 Tokio 使用，端口签名明确依赖内层数据；
混合文件拆成 domain 值定义和 model 端口定义。Rust 导入路径变化，现有 JSON 字段、
默认值、持久文件格式、协商规则和连接预算保持兼容。

验证包含全 workspace 全目标编译与 clippy、格式和文档链接检查，以及迁移类型的
序列化/不变量测试、连接协商测试、运行资源与数据的协作测试、相关消费者和依赖守卫的
定向测试。工作区全量测试和覆盖率留到提交准备阶段。
