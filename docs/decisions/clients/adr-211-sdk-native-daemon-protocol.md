# ADR-211：SDK 原生使用 daemon 协议

- 状态：Accepted。
- 日期：2026-11-04。
- 修订：[ADR-044](adr-044-paseo-client-rust-transport.md)、[ADR-094](adr-094-canonical-ait-client-methods.md)。

## 背景

Ait 只提供 Rust daemon。迁移 Paseo 时，为保留 SDK API，在应用的 `runtime/rust-daemon`
增加了协议适配器。方法名统一后，SDK 仍先序列化 `session` 封装，适配器再解析并生成
Rust v1 请求；返回路径执行相反转换。共享 SDK 的实际运行因此依赖应用源码，并存在
SDK 等待者结束后协议层仍保留请求关联和排队重试的情况。

## 决策

`@ait/client` 直接拥有 Rust v1 协议。`DaemonClient` 创建内部协议连接，将业务请求对象
直接交给协议模块，在物理传输边界序列化一次。返回帧解析为 SDK 类型投影，继续执行
现有 schema 校验，不再序列化和解析中间 `session` 消息。

握手、能力目录、请求关联、订阅物理归属、错误投影、消息分块和上传确认归 SDK。
已有 SDK 等待者的请求使用等待者的超时和取消生命周期，同时清理协议关联及 admission
重试；无等待者的请求保留有界清理。业务写入不因断线自动重放。

`transportFactory` 只创建收发原始帧的连接。应用 `runtime/daemon-transport` 负责选择
Electron IPC、原生 WebSocket、浏览器票据和账号 Hub。直连与 SSH 保留四通道，Hub
保留单会话；通道数调整不属于本次变更。共享 SDK 及 E2E/Maestro 调用方都无需协议包装。

SDK 的业务函数、事件类型投影和高层兼容 API 名称保持稳定。原始 Paseo wire 不再是
客户端模式。Electron 的本机 daemon 进程管理、Rust 业务服务和服务端协议保持原有归属。

## 后果与验证

协议测试跟随实现移动到 `packages/client/src/protocol`；SDK 业务单元测试在协议连接边界
使用测试替身，实际 wire 与客户端整合由协议测试验证。平台连接测试仍由应用维护。
新增公共 SDK 原生握手与鉴权、请求超时清除重试及关联的回归验证。方法目录审计改为读取
SDK 内的声明，并继续与 Rust 所属组件的声明核对。

相关验证包括 SDK 构建、客户端及平台定向测试、类型检查、方法审计、格式、lint 和文档链接。
未修改 Rust 代码，不运行 Rust 测试或工作区覆盖率。
