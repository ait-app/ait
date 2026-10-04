# ADR-084：iOS 在线服务账户与主机中继

- 状态：已实现
- 日期：2026-10-05
- 关系：将 [ADR-076](adr-076-android-account-relay.md) 的原生客户端适配扩展到 iOS，沿用 [ADR-083](adr-083-online-service-host-sync.md) 的登录与逐主机同步边界。

## 背景

0.0.17 的 iOS 客户端没有在线服务入口。共享界面依据平台能力隐藏入口；账户会话、原生票据 WebSocket 和下载只接入 Android。iOS 因而无法登录、发现在线电脑或通过账户中继连接。

## 决策

- Android 与 iOS 共用一个原生账户管理器。安装标识保存在 AsyncStorage，会话凭据保存在 Expo SecureStore；注册节点时分别标识平台与设备名称。两平台使用相同的前后台暂停、恢复、会话清理和传输关闭逻辑。
- iOS 与 Android 共用原生 fetch、带 Authorization 头的一次性票据 WebSocket、Relay 配对与身份校验，以及逐块写入缓存文件并调用系统分享的下载流程。Android 专用 WebSocket 配置插件继续处理 Android 自动附加的 Origin；iOS 的 React Native WebSocket 请求直接转发票据头。
- 欢迎页、添加连接、应用在线服务设置和主机同步入口在 iOS 上启用。账户发现仍不自动连接所有在线电脑；用户显式选择一台后才加入 HostRuntime。逐主机同步只对连接中的 daemon 执行，客户端手机不作为工作主机发布。
- Web 浏览器继续使用直接连接。桌面仍由主进程持有账户凭据和中继传输；iOS 客户端不会获得桌面 IPC 能力。

## 后果

iOS、Android 与桌面客户端均可登录在线服务并选择在线电脑。iOS 使用已有的 Expo 原生依赖，无需新的服务端协议或 Rust 代码。发布此能力需要重新构建 iOS 安装包；在真机上的账户到远程电脑端到端连接仍需验收。
