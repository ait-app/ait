# ADR-182：客户端接入 Hub 账号服务

状态：已实现。日期：2026-10-22。

修订：2026-10-26，桌面内置 daemon 默认选择中心原生通知，独立 daemon 保留显式开启策略。

## 背景

[ADR-181](../daemon/adr-181-hub-host-publication.md) 让 Host 发布事件和目录摘要。
客户端此前只有原生移动端使用 Hub 会话，设备登记、活动报告和列表仍依赖逐 Host 连接。
中心的账号方法与 daemon 同名方法有不同的参数和结果，客户端必须显式接入。

## 决策

- `packages/client/src/hub` 统一编解码 call/reply/event，读取 `welcome.center_methods`，
  调用与会话共享序号及 ACK/replay。每条链路最多 16 个未完成调用和 2 MiB 参数，
  30 秒结果超时；ACK 只表示接纳，reply 才表示业务完成。物理续传保留调用，逻辑链路重置使调用失败。
- 原生移动端、Electron main、托管浏览器各自拥有账号的 access 链路。桌面和托管浏览器
  会话按 presence 优先连接 Hub，初始等待有界；目标缺少 serve 或中心不可用时使用现有 Relay。
  已建立的会话在逻辑链路内续传，普通详情、写入、终端和文件仍发送给对应 Host。
- 既有账号主机列表合并同一服务的 Hub presence；Hub-only Host 的连接资格不依赖旧 Relay
  的 `instance_id` 或 `relay_modes`。presence 消失后回到旧 Channel 状态，不改写旧 Relay 元数据。
  Hub 按已认证 serve node 路由，accept 仍校验账号所记录的 `server_id`、有效实例与 Ait 协议；
  旧 Relay 继续校验 visit 票据绑定的实例。
- Electron renderer 通过 `account_hub_status`、`account_hub_call` 与账号事件 IPC 使用服务。
  主进程只允许 access 方法，保留错误 code；订阅归创建窗口，销毁或主文档导航释放兴趣。
  同一 access 链路汇总窗口心跳，可见窗口优先，隐藏窗口不会覆盖另一窗口的前台状态。
  通知在主进程按 attention 身份去重，并抑制其他窗口正在查看的资源提醒。
- 共享界面挂载一个账号生命周期，设备通过中心 `push.register` 注册一次，不依赖任何 daemon
  视图；前台恢复、token 变化、链路重建会重新登记，显式退出先撤销再释放账号。
  既有逐 Host 登记继续服务直连、旧中心和未启用中心通知的 Host，Host 选择其通知发布路径。
- 桌面内置 daemon 启动时默认传入 `AIT_SERVER_HUB_NOTIFICATIONS=true`，并透传该变量的
  显式设置，包括 `false` / `0`。其他 `AIT_SERVER_*` 环境仍按桌面运行时隔离规则处理。
  中心未声明事件发布能力时继续原生直发路径；独立 daemon 沿用 ADR-181 的默认关闭策略。
  运行中或退出后保留的 daemon 须重启后采用新配置，中心提供方凭据和设备登记仍是投递前提。
- 账号心跳带当前路由对应的 Host 和 Agent/terminal 焦点，每 15 秒报告。
  完成事件订阅 bootstrap 后才报告可见，避免尚未接收 attention 时抑制原生推送。
  账号 attention 与已有直连事件共享语义去重键，原生后台仍由 APNs/FCM 投递。
- 订阅以业务 cursor 补拉：先取 cursor、读取摘要，再分页订阅至 active；
  成功处理事件后才推进游标。物理重连保留兴趣，新逻辑链路刷新摘要后从已消费游标补拉；
  replay 过期重新建立基线。消费队列最多 256 条，超出后释放并从已消费游标恢复。
  游标仅保留于当前界面生命周期，冷启动重新获取摘要与基线。
- 客户端始终使用当前登录的一个账号，保留已有账号和主机列表及导航结构。
  Hub 的资源索引用于内部数据接入，不增加账号层级、独立资源浏览页或额外入口。
  通知生命周期只读取 Agent 摘要，用于通知标题、Workspace 路由和 attention 过期判断；
  读取按 `(hostId,id)` 区分同 ID 资源，并完整分页后替换内存快照。

## 后果

账号通知可在没有打开 Host 视图时工作；通知所需摘要保存在内存，退出或切换服务即清理。
中心需要部署账号服务及通知提供方凭据；桌面内置 Host 默认将后台原生通知交给支持相关能力的中心，
独立 Host 仍须按 ADR-181 显式启用 `AIT_SERVER_HUB_NOTIFICATIONS=true`。
旧中心继续提供已保存主机的连接。
时间线正文、审批操作和创建详情由 Host 负责，中心的轻量 marker 可作为额外消费者的失效通知。
