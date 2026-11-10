# 账号登录与主机上线

Desktop、Android 与 iOS 使用浏览器统一登录；网页客户端共享同源登录。
用户会话、daemon 机器授权和本地 API token 是三种独立凭据，分别用于账号访问、主机发布和直接连接。

## 登录

1. 在欢迎页、**Settings → App → Online Service** 或 **Add Host → Online Service** 点击登录。
2. Desktop/Android 打开系统浏览器，iOS 使用系统认证窗口；按中心页面完成登录、注册或账号绑定。
   可用登录方式由服务端返回，客户端不保存 Authing App Secret。
3. 在浏览器确认并返回客户端；客户端保存 Ait 会话并加载在线主机。
4. 选择主机并添加到本地列表。已添加记录会保留，恢复账号后重新连接；保存记录不等于主机在线。

默认 API 为 `https://aitapp.cn/api`；客户端 Service settings 可指定兼容服务的 API 基础地址，
`/hosts` 或 `/dash` 管理页面不是 API 地址。daemon 的机器授权中心固定为生产地址，
不会随客户端的自定义服务设置变化，见[无人值守 daemon](headless-daemon.md)。

原生客户端只保留浏览器统一登录，服务不支持时提示升级，不回退到旧邮箱密码表单。
账号信息显示的到期时间来自用户资料，与当前登录会话的到期时间不同。
服务可通过 `GET /v1/auth/providers` 广告 `authing_enabled` 和 `native_login_enabled`；
实际启用的登录方式、账号期限与中心部署配置由服务维护者确认。

## 主机发布与退出

macOS/Linux Desktop 默认尝试授权内置 daemon；Windows 没有内置 daemon。
其他主机可在 **Host → Connections → Sync with online service** 显式连接。
客户端批准 device authorization 后交给 daemon，由 daemon 自行领取机器凭据、注册节点、续租及重连。
客户端只维护自己的 access 节点和账号连接。

| 操作                                | 结果                                                                                |
| ----------------------------------- | ----------------------------------------------------------------------------------- |
| 关闭客户端                          | 保留用户凭据；daemon 授权不撤销，但进程是否继续运行取决于桌面退出设置或服务管理方式 |
| 退出客户端账号                      | 清除该客户端用户凭据；不撤销 daemon 的机器授权                                      |
| 主机设置中停止在线同步 / 完整断开   | 关闭 Hub 与下载 Relay、停止续租、清除机器授权；重启不会自动恢复                     |
| 重新启用主机同步或执行 `aitd login` | 显式重新授权；CLI 登录前先停止同目录 daemon                                         |
| 从客户端删除已添加主机              | 移除本地连接记录，不等于撤销机器授权                                                |

显式停止过的主机不会在重新登录 Desktop 时自动授权。机器授权的持久化、撤销和恢复细节见
[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)。在线业务使用 Hub，独立下载与账号状态通道
继续各自运行，见[连接诊断](relay-diagnostics.md)。

## 登录保持

共享客户端按中心返回的 `expires_in` 保存 `expiresAt`；当前没有用户会话自动续期。
Desktop 使用系统安全存储，Android/iOS 使用 SecureStore；Desktop 没有可用安全存储时不落盘凭据。
Web 使用持久存储恢复有效登录，禁止存储时只能保留当前页面会话。
关闭应用不主动删除凭据；主动注销、明确授权失效或会话到期后需要重新登录。

不要把历史“最长 14 天”部署值当作客户端保证，也不要把账号有效期当作登录有效期。
期限调整记录见[旧登录保持方案](../archive/plans/relay-client-session-persistence.md)，
尚未实施的轮换协议见[客户端续期待办](../plans/relay-client-session-persistence.md)。
机器 refresh token 只由 daemon 使用，不能拿来延长用户登录。

## 构建与验收

iOS 登录依赖原生认证模块，变更原生依赖后需要重新生成工程并安装构建，不能只更新 JavaScript。
本机构建见 [Apple 构建](apple-builds.md)，CI 分发使用[发布指南中的 Xcode Cloud 流程](releasing.md#apple-testflight-ios-内部测试)。

验收应记录客户端和中心版本，覆盖登录回传、取消、超时、系统认证拒绝、进程重开、断网恢复、
会话到期和主动退出；同时检查退出账号后 daemon 仍按自身授权运行、显式断开后重启不会重新发布。
实际第三方登录、系统回传和安全存储需在对应设备验证，模拟测试不能替代。
不要在日志中记录一次性代码、verifier 或登录令牌。

早期 Authing/手机号数据库迁移与历史验证保留在[旧部署记录](../archive/operations/authing-client-login.md)，
本仓库的文档整理没有验证当前线上中心配置。
