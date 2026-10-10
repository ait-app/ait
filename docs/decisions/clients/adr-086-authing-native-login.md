# ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录

- 状态：已实现；桌面已完成真实 Authing 登录联调，移动端待安装包验收
- 日期：2026-10-06
- 关系：扩展 [ADR-074](adr-074-account-host-relay.md)、[ADR-076](adr-076-android-account-relay.md)、[ADR-084](adr-084-ios-account-relay.md)；保留 [ADR-083](adr-083-online-service-host-sync.md) 的登录与主机同步边界。

## 背景

在线服务新增 Authing 托管注册、邮件验证、密码找回及社交登录，原客户端只有本地邮箱密码接口。Web 控制台使用浏览器 Cookie 绑定的完成票据，不能直接交给原生客户端兑换。

## 决策

- 共享账户管理器读取服务的 `/v1/auth/providers`。服务同时声明 `authing_enabled` 和 `native_login_enabled` 才能发起统一登录。按 [ADR-123](adr-123-browser-only-account-login.md) 修订，客户端仅显示统一登录入口，删除旧密码入口和调用路径。客户端不配置 Authing App Secret。
- 桌面在主进程生成随机 state 和 PKCE verifier，临时监听 `127.0.0.1` 的随机端口，再打开系统浏览器。Android 使用 Expo Crypto 生成随机值，通过系统浏览器登录，监听已注册的 `ait://auth/callback` 应用链接。iOS 使用 Expo WebBrowser 的 `ASWebAuthenticationSession` 系统认证窗口，由会话返回同一应用链接，不额外监听 Linking；共享回调解析器继续校验完整地址与 state。新增 `expo-web-browser` 原生模块，安装包需重新构建。
- Authing 仍回调中心服务的现有 HTTPS 地址。中心验签、查找本地账号后，浏览器显示账号邮箱或手机号，由用户确认登录客户端。确认票据绑定原浏览器 Cookie；随后生成 60 秒有效的一次性代码。客户端验证回调地址和 state，并提交本次 verifier 换取 AIT 业务 JWT。JWT 不进入浏览器跳转 URL。
- 登录尝试最多等待 10 分钟，可主动取消；移动端暂时切到浏览器或后台不会取消尝试；iOS 关闭认证窗口会结束尝试，客户端取消或超时会关闭认证窗口。verifier 只保存在进程内，应用进程被系统结束后需重新发起登录。回调只导航回账户设置，路由层不处理凭据。
- AIT 会话继续由桌面 safeStorage 或移动端 SecureStore 保存，桌面渲染进程只接收非秘密快照。账户有效期与会话有效期独立：新 Authing 客户默认 7 天，反复登录不续期；会话到期后重新认证。
- 保留客户端退出的既有语义：清理本机会话和绑定 daemon 的租约；其他显式同步的 daemon 继续按原授权续租。此操作不调用中心全会话登出，也不清除 Authing 浏览器会话。中心验证本次 OIDC 响应、nonce 与签发时间，并要求浏览器确认当前账号；`prompt=login` 不保证提供商重新校验凭据，不把它视为强制新鲜认证。账号过期、停用或中心撤销会话继续由服务端阻止访问。
- 桌面、Android 和 iOS 都启用统一登录；浏览器连接模式不增加账户权限。

## 后果

共享账户发现、中继访问和逐主机同步可复用原逻辑，认证结果最终仍是 AIT JWT。中心必须先部署配套 native handoff 接口、`0008_authing.sql` 迁移和浏览器确认页；客户端不会把缺少接口的旧中心误判为支持统一登录。

配置与验收见[客户端统一登录](../../operations/authing-client-login.md)。原生浏览器与回环回调遵循 [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html) 的模式；中心到客户端的一次性兑换是 AIT 自有协议，不是直接使用 Authing 原生应用凭据。

## 2026-10-10：手机号账号

中心认证的联系方式可以是已验证邮箱或手机号。客户端登录响应的 `email` 可空，使用可选 `phone_number` 作为联系方式回退，且仍拒绝两者均缺失或无效的响应。验证号码、关联身份和撤销会话由中心负责；共享账户管理器继续只消费中心签发的会话。此扩展不改变桌面与移动端的回传、密钥存储和主机同步边界。部署顺序及设备验收见[客户端统一登录](../../operations/authing-client-login.md#手机号账号兼容2026-10-10)。
