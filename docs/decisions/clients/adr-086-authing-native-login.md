# ADR-086：桌面与 Android 的 Authing 浏览器登录

- 状态：已实现，待真实 Authing 与安装包联调
- 日期：2026-10-06
- 关系：扩展 [ADR-074](adr-074-account-host-relay.md)、[ADR-076](adr-076-android-account-relay.md)；保留 [ADR-083](adr-083-online-service-host-sync.md) 的登录与主机同步边界。

## 背景

在线服务新增 Authing 托管注册、邮件验证、密码找回及社交登录，原客户端只有本地邮箱密码接口。Web 控制台使用浏览器 Cookie 绑定的完成票据，不能直接交给原生客户端兑换。

## 决策

- 共享账户管理器读取服务的 `/v1/auth/providers`。服务同时声明 `authing_enabled` 和 `native_login_enabled` 才显示统一登录入口；旧密码入口继续保留。客户端不配置 Authing App Secret。
- 桌面在主进程生成随机 state 和 PKCE verifier，临时监听 `127.0.0.1` 的随机端口，再打开系统浏览器。Android 使用 Expo Crypto 生成随机值，通过系统浏览器登录，监听已注册的 `ait://auth/callback` 应用链接。原生登录只依赖已有组件，无新增 Android 原生依赖。
- Authing 仍回调中心服务的现有 HTTPS 地址。中心验签、查找本地账号后，浏览器显示账号邮箱，由用户确认登录客户端。确认票据绑定原浏览器 Cookie；随后生成 60 秒有效的一次性代码。客户端验证回调地址和 state，并提交本次 verifier 换取 AIT 业务 JWT。JWT 不进入浏览器跳转 URL。
- 登录尝试最多等待 10 分钟，可主动取消；Android 暂时切到浏览器不会取消尝试。verifier 只保存在进程内，应用进程被系统结束后需重新发起登录。回调只导航回账户设置，路由层不处理凭据。
- AIT 会话继续由桌面 safeStorage 或 Android SecureStore 保存，桌面渲染进程只接收非秘密快照。账户有效期与会话有效期独立：新 Authing 客户默认 7 天，反复登录不续期；会话到期后重新认证。
- 保留客户端退出的既有语义：清理本机会话和绑定 daemon 的租约；其他显式同步的 daemon 继续按原授权续租。此操作不调用中心全会话登出，也不清除 Authing 浏览器会话；每次统一登录仍要求新鲜认证。账号过期、停用或中心撤销会话继续由服务端阻止访问。
- 这次启用桌面和 Android。iOS 继续原有密码登录，浏览器连接模式不增加账户权限。

## 后果

共享账户发现、中继访问和逐主机同步可复用原逻辑，认证结果最终仍是 AIT JWT。中心必须先部署配套 native handoff 接口、`0008_authing.sql` 迁移和浏览器确认页；客户端不会把缺少接口的旧中心误判为支持统一登录。

配置与验收见[客户端统一登录](../../operations/authing-client-login.md)。原生浏览器与回环回调遵循 [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html) 的模式；中心到客户端的一次性兑换是 AIT 自有协议，不是直接使用 Authing 原生应用凭据。
