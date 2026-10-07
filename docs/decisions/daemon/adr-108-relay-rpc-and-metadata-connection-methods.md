# ADR-108：Relay RPC 与基础连接方法归所属 crate

- 状态：接受
- 日期：2026-10-08
- 修订：[ADR-100](adr-100-crate-level-service-installation.md) 中由 API 声明基础连接方法的归属，
  以及 [ADR-074](../clients/adr-074-account-host-relay.md) 中 relay 不依赖 workspace crate 的约束。

## 背景

API 中的 relay RPC 只控制既有 connector，却接收 API 私有的完整 `Shared` 状态。
九个基础连接方法由 API 声明，但它们的请求处理、订阅和会话基础设施已经位于 metadata。
让声明和处理归同一组件，可以减少宿主中的功能实现，同时保持精简 host 的连接能力。

## 决策

1. relay 拥有 `rpc::METHODS` 和 `rpc::request`，通过 model 的 `MethodSpec`、`Context`、
   请求、输出队列和错误契约处理状态、启动、停止三个方法。入口只接受请求与 `Connector`，
   响应身份来自 connector 构造时固定的 server/instance ID，不引用 API 私有状态。
2. relay 的内部依赖调整为仅依赖 model。继续禁止 relay 依赖 API、metadata、provider、
   protocol 等宿主或功能实现；不接受请求方指定的本地目标地址。
3. metadata 通过 `capabilities::connection_methods()` 声明九个基础连接方法，独立于
   可选的 metadata 业务 `Service`。`implemented_methods()` 与 `installed_methods(bool)`
   继续仅描述业务服务的全量或空方法集合，API 总是汇入基础连接方法及三个 relay 方法。
4. API 保留 HTTP/WebSocket 鉴权、握手、能力校验、连接预算和跨能力协调。HTTP 控制路由与
   WebSocket RPC 使用同一个 connector，构造、停机与取消仍由 API 宿主管理。
   订阅释放的执行组屏障、终端心跳处理及创建记录的 Provider 身份校验继续位于 API。
5. 删除 API 的 `core_methods.rs` 和 `relay_rpc.rs`，调用处直接导入所属 crate 的入口，
   不保留转发模块。

## 后果与验证

功能声明与处理拥有一致的归属，依赖仍无环。方法名、消息方向、响应字段、错误码和
鉴权行为保持兼容；精简 host 仍提供九个基础连接方法与三个 relay 方法，完整宿主的
实现方法和可协商能力集合保持一致。

定向验证涵盖 connector 的公开状态、错误映射、请求所有权与交付失败；API 验证 HTTP/WS
共用实例、鉴权和协商、基础连接行为与跨能力订阅释放。依赖守卫验证新增的 relay → model
边界，另运行 workspace 全目标编译、格式、lint 和文档链接检查。
