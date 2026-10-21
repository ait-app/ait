# ADR-177：移动端本地通知与 APNs／FCM 直连

状态：Accepted（2026-10-21）。

移动端已有完成／权限事件和设备登记，但原生本地通知被禁用，daemon 仅持久化 token，未执行远程投递。现采用前台本地通知、后台由 Apple APNs 或 Google FCM 直接投递；不使用 Expo Push 中转。`expo-notifications` 仅负责原生权限、设备 token、显示和点击回调。

`model::session` 在关注策略判断后发布独立、有界的 attention 通道，连接断开后仍可产生推送。连接仅关联已持久化的设备 token；新鲜的前台连接由本地事件处理，后台和断线设备由远程投递处理。心跳接收时间决定连接是否新鲜，用户最近活动时间继续决定本地提醒接收者；正在查看对应 Agent／终端时抑制提醒。

`metadata::push` 拥有内容、活跃租约选择、并发限制、重试及无效 token 清理。`PushTransport` 是该能力的端口；`daemon::host::push` 实现 APNs HTTP/2、Apple ES256、FCM HTTP v1 和 Google 服务账户 OAuth。Provider 和 Terminal 继续只发布已提交事件，不依赖 metadata 或 HTTP adapter。API 持有投递任务的生命周期，停机取消并等待受跟踪的存储操作。

现有 `push.register` 字符串字段和租约文件格式保持兼容，新客户端将 provider、原生 token、APNs Bundle ID 与真实签名环境序列化为安装描述符。旧 Expo token 不会继续投递并会被清理。确认设备失效后，清除租约及连接 token 所有权，旧心跳不能复活它；显式重新登记可以恢复。

队列最多保留 64 个事件，每个事件最多并行投递 6 个设备。可重试错误最多尝试 3 次，429 尊重有界的 Retry-After。通知为尽力投递，队列和重试不跨 daemon 重启持久化，不承诺设备已展示或恰好一次。凭据由受信任的投递主机通过环境指定的私有文件读取，不打包进客户端。没有 provider 凭据时仍提供本地通知。

移动端设置显示系统权限并支持授权、打开系统设置及本地测试。授权不再依赖 token 缓存读取成功；回前台、重连及原生 token 轮换可恢复登记。配置和验收见[移动端通知](../../operations/mobile-notifications.md)。
