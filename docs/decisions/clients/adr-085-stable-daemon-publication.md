# ADR-085：Daemon 同步的稳定节点身份

- 状态：已实现
- 日期：2026-10-05
- 关系：补充 [ADR-083](adr-083-online-service-host-sync.md) 的逐主机注册身份与旧数据兼容。

## 背景

每次开启主机同步都会生成新的 `installation_id`，但中心数据库将同一 Host 永久绑定到
唯一节点。停止同步只关闭租约，因此再次同步、daemon 重启或客户端重启后，新节点
绑定同一 Host 会触发 `Resource already exists`。旧版桌面自动发布留下的绑定也会冲突。
若用户删除或禁用旧 Host，节点上残留的绑定还会使纯客户端登录后的注册返回 `403`，
违背账户登录与主机同步分离的既定行为。

## 决策

- 显式 daemon 发布使用 daemon 的稳定 `server_id` 作为 `installation_id`。客户端安装
  仍使用自身持久 ID；主机租约继续独立于客户端登录租约。
- 新租约生成新的 `registration_id`。注册失败返回 `registration_expired` 时替换该 ID，
  由下一次同步重试；普通网络错误或活动租约冲突保持原 ID。
- 中心 `ait-server` 在同一注册事务内转移旧 Host 绑定到稳定 daemon 节点。旧节点没有
  有效 runtime 租约时才允许转移，原 Host ID 保持不变，旧纯客户端租约继续有效。
  禁用、删除的节点或 Host 不允许迁移；活动 runtime 必须先停止同步或等待租约到期。
- `runtime = null` 的客户端注册可以解除指向已禁用或已删除 Host 的旧绑定，并撤销旧
  runtime 租约。原客户端节点和 Host 删除记录保留，客户端可以发现和访问其他主机；显式 daemon 发布继续
  拒绝启用该 Host，节点自身的禁用/删除状态仍阻止注册。
- 通用 Electron 调用通道由 `paseo:invoke` 改为 `ait:invoke`，preload 和主进程同步切换。
  该通道不对外提供 daemon 协议，也不持久保存。

## 后果

同一账号下，不同客户端对同一 daemon 使用相同节点身份；同时只有一份有效同步租约。
现有旧绑定需要部署中心兼容修复。新客户端连接尚未升级的中心时，已有绑定仍可能冲突。
中心无需改表或删除用户数据。

测试及定向覆盖率见[验证报告](../../reports/clients/daemon-registration-validation.md)。
