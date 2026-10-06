# 桌面与 Android 统一登录

## 服务端准备

先发布配套 `ait-server` 后端和 Web 控制台，运行 `0008_authing.sql`。保持原有 Authing HTTPS 登录回调配置；桌面的回环地址和 Android 的应用链接由 AIT 中心处理，无需加入 Authing 应用回调白名单。

检查实际服务地址：

```sh
curl https://dash.ait-app.com:8443/api/v1/auth/providers
```

响应须包含 `authing_enabled: true`、`native_login_enabled: true`。客户端不填写 App ID 或 App Secret，开发环境可在「在线服务 → 服务设置」填写对应 API 地址，例如 `https://ait.h.stdin.in:8443/api`。

## 用户流程

1. 在「在线服务」点击「登录 / 注册」，系统浏览器打开 Authing 页面。
2. 完成邮箱、Google 或微信登录。注册、邮箱验证和密码找回在托管页面完成。
3. 浏览器显示当前邮箱，点击「确认并返回客户端」。Android 如未自动唤起，点击页面上的「返回 AIT」。
4. 客户端保存 AIT 会话，显示账户有效期并加载在线主机。新账号默认创建 7 天后到期，管理员续期后可重新登录。

同邮箱已有旧账号时，先使用原密码登录 Web 控制台，在设置里绑定统一登录；绑定保留原 Host、角色和有效期。客户端保留「使用原 AIT 密码登录」入口。

客户端退出保持已有逐主机同步语义，不等同于中心的全会话注销。详情见 [ADR-086](../decisions/clients/adr-086-authing-native-login.md)。

## 联调验收

- 安装新版桌面与 Android 客户端，分别完成邮箱、Google、微信登录及新账号注册。微信需提供已验证邮箱。
- 验证取消、超时、用户关闭浏览器、Android 切到后台再返回、应用进程被结束后的重新登录。
- 验证旧密码登录、旧账号显式绑定、主机发现与访问，以及账户到期后的续期提示。
- 登录尝试、一次性代码和 verifier 不得写入日志；客户端只持久保存 AIT 会话。真实 Authing、系统浏览器唤起和安装包回传需在对应设备上验收，模拟测试不替代此步骤。

## Test coverage

此次客户端只修改 TypeScript/React，无 `bins/` 或 `crates/` Rust 行为变更，Rust 覆盖率不适用。TypeScript 行覆盖率未测量；本地迭代运行共享会话、桌面回环回调、Android URL 回调、安全存储与登录界面的定向测试。真实提供商和设备端到端路径仍需按上述清单验收。
