# 移动端通知

Android 与 iOS 在 App 前台、连接仍能收到事件时，直接发起本地完成、失败和权限提醒；正在查看对应 Agent 时不提醒。后台或锁屏后的提醒由连接的 daemon 直连 Apple APNs／Google FCM。终端 hook 的 attention 使用相同链路，点击通知打开对应 Host／Workspace／Agent 或终端。

App 的 **Settings → App → Notifications（设置 → 应用 → 通知）** 显示系统权限，可以请求授权、打开系统设置或发送本地测试。测试只验证系统显示，不验证远程 provider。首次连接主机后自动请求权限；此前拒绝过的权限须在系统设置修改，回前台重新登记。

## iOS：Apple APNs

原生构建须带有 Push Notifications entitlement 和匹配的 provisioning profile。客户端读取真实 `aps-environment` 和 Bundle ID，选择 sandbox 或 production；TestFlight／App Store 使用 production。新增 `expo-application` 后须重新生成原生工程并构建安装包，不能仅依赖 JS 更新。

在运行 daemon 的受信任主机中配置：

```sh
export AIT_APNS_KEY_FILE=/private/path/AuthKey.p8
export AIT_APNS_KEY_ID=YOUR_KEY_ID
export AIT_APNS_TEAM_ID=YOUR_TEAM_ID
export AIT_APNS_BUNDLE_IDS=com.necokeine.ait,dev.ait.mobile,dev.ait.mobile.debug
```

密钥须允许目标 Bundle ID 的推送。Bundle allowlist 默认为上述三个应用：已提交原生工程的 `com.necokeine.ait`，以及 Expo 配置默认的 production／development ID。自定义构建须显式设置；设置后覆盖默认列表。daemon 缓存 ES256 provider token，50 分钟更新，并使用 HTTP/2。凭据文件不能提交到仓库或放进公开安装包。

## Android：Google FCM

原生构建须提供与应用匹配的 `google-services.json`，并在 Google 项目启用 Firebase Cloud Messaging API。daemon 使用具有 FCM 发送权限的服务账户：

```sh
export AIT_FCM_SERVICE_ACCOUNT_FILE=/private/path/firebase-service-account.json
```

也支持 `GOOGLE_APPLICATION_CREDENTIALS` 指向服务账户 JSON。项目 ID、邮箱和 RSA 私钥从文件读取；通过 Google OAuth 获取并缓存访问 token，通知经 FCM HTTP v1 发送。当前一个 daemon 配置一个 Firebase 项目。F-Droid 构建仍不接入 Google 推送。

## 验收与故障排查

1. 更新 daemon 和原生 App，连接主机并允许系统通知。
2. 在通知设置发送本地测试，应显示系统提醒；系统专注模式和显示偏好仍有效。
3. 前台打开另一个对话，让后台 Agent 完成，应收到本地提醒；正在查看该 Agent 时不提醒。
4. 切后台或锁屏，让电脑上的 Agent 完成、失败或请求权限，应收到 APNs／FCM 提醒。手机连接已关闭也可投递，登记租约有效期为 48 小时。
5. 点击通知，确认打开对应对话；终端提醒应打开对应终端标签页。
6. 在系统设置关闭／重新允许通知，再回 App，确认权限状态及设备登记恢复。

没有 APNs／FCM 凭据时，本地测试和前台提醒可用，后台远程提醒不可用。队列有界且不跨 daemon 重启持久化，provider 接纳不代表设备已展示。网络故障最多尝试 3 次；明确失效的设备 token 会持久删除，认证和项目配置错误不会删除有效设备。

服务端只记录安全错误类别和丢弃计数，不记录设备 token、JWT、OAuth token 或 provider 原始错误正文。客户端 `[PushNotifications]`／`[Notifications]` 日志可区分权限、设备 token 与本地显示故障。

接口依据：[Apple APNs](https://developer.apple.com/library/archive/documentation/NetworkingInternet/Conceptual/RemoteNotificationsPG/CommunicatingwithAPNs.html)、[FCM HTTP v1](https://firebase.google.com/docs/cloud-messaging/send/v1-api)、[Google 服务账户 OAuth](https://developers.google.com/identity/protocols/oauth2/service-account)。实现决策见 [ADR-177](../decisions/clients/adr-177-native-local-and-remote-notifications.md)。
