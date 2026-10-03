# ADR-076：Android 账户与中继客户端

- 状态：已实现
- 日期：2026-10-03
- 关系：扩展 [ADR-074](adr-074-account-host-relay.md) 的客户端范围，将账户状态机迁入共享 SDK，保留桌面主进程凭据边界。

## 背景

[ADR-074](adr-074-account-host-relay.md) 的账户入口和中继传输依赖 Electron IPC，
共享界面虽然包含账户组件，Android 构建仍无法登录或连接账户主机。本决策扩展客户端适配，
沿用账户中心和 Rust daemon 的现有协议。

## 决策

- 将账户登录、节点租约、发现和按需授权状态机移到 `packages/client`。平台注入设备身份、
  UUID、安全存储和本地运行时操作。Electron 保留原有主进程凭据边界。
- Android 使用 Expo SecureStore 保存 JWT、到期时间及原节点会话；密码只用于登录请求。
  AsyncStorage 仅保存稳定安装标识。默认中心与桌面一致，自定义中心要求 HTTPS；仅本机开发
  允许 HTTP。使用 Expo 原生 fetch，保留禁止认证请求跟随重定向的语义。
- Android 注册 `runtime: null` 的客户端节点，不启动 daemon 或主机控制连接。独立轮询在线
  电脑；显式选择后才把该主机加入 HostRuntime，发现不会自动探测每台电脑。
- 原生 WebSocket 通过 Authorization 头传递一次性票据。Expo 插件仅对带 Bearer 的中继
  `/v1/relay/sessions/{uuid}/client` 请求移除 React Native 默认添加的 Origin，匹配无浏览器
  Origin 的票据绑定；禁止 WebSocket 重定向。浏览器鉴权协议保持独立。
- 中继先校验 `relay.ready` 的会话 ID，再发送 Rust hello，校验返回的 server ID、instance ID
  和 `ait-rust-single-v1` feature。业务复用现有 Rust 单连接 adapter；帧限制为 1 MiB，
  连接设置有超时，不重放业务写入。
- 下载使用独立 `ait-download-v1` 会话，逐块写入原生临时文件，不在 JS 内累计整个文件。
  校验配对、响应头、长度及结束消息，完整接收后确认并移动文件，再提供系统分享。
  失败清理部分文件；退出账户、切换主机和进入后台关闭现有中继与下载。
- 进入后台停止账户轮询并取消请求，前台重新验证租约和发现；租约已过期时重新注册。
  账户会话与原生生命周期监听均为进程单例。

## 后果

Android 和桌面可通过同一账户访问在线电脑；手机作为客户端，不发布工作主机。
浏览器和 iOS 仍未启用账户登录。Android 增加原生安全存储依赖和 Expo 配置插件，升级该
适配需要重新生成并编译原生安装包。Rust crate 的依赖边界和业务能力归属不变。

自动化验证覆盖账户注册/恢复/续租、前后台切换、凭据存储、Android 界面入口、配对与身份
拒绝、连接取消以及下载完成/失败。只运行变更直接相关的客户端测试；本次没有 Rust 改动，
未运行 Rust 测试或全仓覆盖率。Android 原生构建已验证；实体 Android 设备已确认欢迎页
账户入口与邮箱/密码登录表单，真实账户中继流程尚未完成端到端验收。
