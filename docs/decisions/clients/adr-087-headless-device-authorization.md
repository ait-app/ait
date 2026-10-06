# ADR-087：中心设备管理与 Linux daemon 的 JWT 授权

- 状态：已采纳；本分支实现，待两端发布
- 日期：2026-10-06
- 源码基线：Ait `feat/authing-native-login` / `384f8735`；配套中心 [ait-server PR #9](https://github.com/ait-app/ait-server/pull/9)
- 关系：扩展 [ADR-086](adr-086-authing-native-login.md) 的 OIDC 登录；沿用
  [ADR-085](adr-085-stable-daemon-publication.md) 的稳定身份和
  [ADR-075](adr-075-relay-protocol-modules.md) 的 relay 传输边界；为
  [ADR-083](adr-083-online-service-host-sync.md) 增加由 daemon 自主管理的主机发布模式。

## 背景

Linux 服务器需要在没有桌面界面、用户退出 SSH、客户端关闭及机器重启后，独立保持到
relay 的连接。当前 Rust daemon 已有运行时和 relay 通道，但注册与租约续期由客户端账户
管理器负责。现有 Authing 登录最终取得用户 JWT，其 OIDC 会话最长 8 小时，不能直接作为
长期设备授权。

本轮确定由中心管理设备，支持交互式 Web 授权和 CLI 自带 token 授权，由网页服务的后端
签发 JWT 给 Rust daemon。中心固定为 `https://dash.ait-app.com:8443/api`，不允许用户修改。
安装与服务管理由既有脚本负责；浏览器负责用户认证和授权确认，签名密钥只属于中心后端。

## 决策

1. `daemon login --name build-linux` 发起交互式 Web 授权，显示认证链接和设备验证码。
   用户完成现有 OIDC 登录、确认账号与主机后，daemon 轮询领取授权。另支持
   `daemon login --token <TOKEN> --name build-linux`，脚本可使用 `--token-stdin`。
   自带 token 采用现有 Host 页面签发的一次性接入 JWT，兑换后与 Web 模式共用设备
   access JWT 和 refresh 流程。Web 回调仍到中心；设备确认借鉴
   [RFC 8628](https://www.rfc-editor.org/rfc/rfc8628.html)，不依赖 Authing 提供设备授权端点。
2. 中心保存独立设备授权，签发短期设备 access JWT 和轮换的 opaque refresh token。
   API 访问凭据是 JWT；refresh token 仅用于中心续期与本设备撤销。OIDC 会话自然到期和
   普通网页登录退出不撤销设备授权；账号禁用、账号到期及显式设备撤销仍阻止访问。
3. 设备 JWT 采用独立 audience、明确 token 类型、独立签名密钥和最小权限，只能发布自身
   Host、维护自身节点租约和申请控制票据。现有用户 JWT 接口不得接受设备 JWT。
   TTL、claims、轮换与撤销规则见[详细设计](../../plans/linux-headless-device-auth.md)。
4. `server_id` 持久化，`installation_id = server_id`，`instance_id` 随运行实例变化。
   复用 `hosts / nodes / node_sessions`，新增 `node_grants` 与现有 Host、节点及所属账号绑定；注册和续租不能改变授权的目标。一个主机同时
   只有一个活动 runtime，沿用现有冲突与失效租约处理。
5. 节点会话记录`device_grant_id` 授权来源。刷新 JWT 后可推进同一会话的授权截止时间，
   不以 JWT 的 `jti` 作为节点会话身份。relay 持续校验中心状态，撤销不能只等待 JWT 到期。
6. 新增 `host-link` crate 承担设备认证、刷新、注册与续租协调，通过 ports 访问凭据存储、
   中心 HTTP 和 relay 控制。`daemon` 组装适配器；`api` 实现 `ManagedRelay` port 并继续持有 `relay`，`relay` 继续
   不依赖其他 workspace crate，仅接收短期一次性票据。`daemon → host-link` 与 `api → host-link` 已纳入依赖守卫。
7. relay 增加 `external` 和 `managed` 所有权模式。无人值守 daemon 由内部管理器持有
   控制权，外部客户端不能再次发布或覆盖控制授权；旧桌面管理模式继续保留。
8. 沿用 `daemon` 二进制，只增加授权、运行和状态相关命令。既有安装脚本负责安装二进制、
   配置服务、启停和卸载；daemon 不提供服务安装子命令。systemd 托管进程，Rust 负责
   网络恢复；后台进程不自动发起交互登录。本地默认只监听 loopback。
9. 无人值守模式所有认证、续期和 relay 地址从固定中心派生，不提供 `--center`、环境变量
   或配置文件覆盖，不接受 token 或响应指定其他中心。测试使用内部依赖注入；已存在的
   桌面/移动客户端服务地址配置保持其既有行为。

## 后果

用户在首次绑定或授权失效后，通过网页登录或提供接入 token 授权，daemon 随后独立续期
并保持在线。中心成为设备授权、撤销和审计的权威；中心不可用时不能创建新远程访问，
已失去有效授权的通道关闭，
本地任务沿用现有生命周期。

实现需要同时修改 Ait Rust daemon、客户端同步入口，以及另一个仓库中的中心 API、迁移
和 Web 设备页面，并与既有安装脚本对接。JWT 的引入不会免除中心数据库状态校验；轮换的
持久化与失败恢复属于首版要求。中心与 daemon 的接口已对齐；上线需要合并两端并开启中心功能，不能视为当前已部署功能。

完整流程、中心契约、恢复规则和发布验收见[Linux 无人值守设备授权设计](../../plans/linux-headless-device-auth.md)。

## Test coverage

实现测试与覆盖率在本分支 PR 中记录；提交前测量 workspace 并提供可 review 的 HTML artifact。
真实固定中心、systemd 与跨设备浏览器验收待两端部署；不能用模拟中心测试替代。
