# ADR-181：Host 向 Hub 发布事件与目录摘要

状态：采用。扩展 [ADR-180](adr-180-hub-serve-endpoint.md)。中心契约由 ait-server 的
`docs/hub-services.md` 定义。

日期：2026-10-22。

## 背景

Hub 已提供账号级设备通知、共享订阅和目录查询，但已有 serve 实现只承载客户端业务会话。
Host 必须主动发布已经提交的变化，中心才能进行分发。Host 仍负责判断 attention 是否有效，
并持有实际资源、Timeline、Provider 配置和原生凭据。

## 决策与职责

- `model::hub` 声明中心调用、发布观察者和原子 outbox 存储端口。
- `relay::hub` 在既有 serve WSS 上实现 `call/reply`，通过 `welcome.center_methods`
  发现能力。调用与会话帧共享序号、ACK、重放预算；业务等待独立于网络确认。
- `persistence::storage::hub` 只实现有界 JSON 文件的读取与原子替换，不拥有发布策略。
- `daemon::host::hub` 组装应用流程：事件投影、通知准备、journal、重试、目录快照和停机。
- `api` 将宿主发布器连接到共享 SessionEvents、creation 和 Provider Timeline；Provider
  与 metadata 不依赖 relay。通知准备复用 `metadata::push::delivery::prepare_notification`。

## 发布内容

| 来源                                  | 中心事件                                                 |
| ------------------------------------- | -------------------------------------------------------- |
| Agent / terminal attention            | 原事件名称；可附原生通知                                 |
| 原生审批请求与撤销                    | `agent_permission_request` / `agent_permission_resolved` |
| Agent / Workspace 创建回执            | `creation.update`，不附完整 Agent / Workspace 对象       |
| 已提交 Timeline append / replacement  | `agent.timeline.update`，只含 Agent、epoch、seq / reset  |
| Workspace setup 摘要、server metadata | `workspace_setup_status`、`status.server_info`           |

观察发生在连接订阅及本机焦点筛选之前。中心模式接管 native push 后，本机原生投递不再
接收同一 attention；本机业务订阅仍按现有规则获得状态和应用内提醒。
原生通知不附设备 token，中心从 access 链路上的设备注册选择投递目标。
Agent 通知入队与发送前都检查当前记录，已经读过、归档、删除、internal 或 timestamp
过期的 attention 不补发原生通知。审批请求本身通过事件分发，permission attention 负责
原生提醒，避免两种事件重复产生通知。

目录通过 `hub.directory.publish` 发布 project / workspace / agent 三种完整替换快照。
变更合并 250 ms 后读取已提交 registry；删除或归档通过空快照或缺少对应 ID 表达。
摘要使用明确字段白名单，排除路径、Provider 配置、原生会话句柄和完整对话。
同一 node session 内 revision 持久递增；内容未变时保留版本与事件 ID。
新逻辑链路重新发布快照，物理续传不重新增加 revision。

## 可靠性与边界

- 网络调用上限 16 个、合计参数 2 MiB；单次业务等待最多 30 秒。逻辑链路更换后未完成调用
  返回 unavailable，由应用使用原业务 event ID 重试。
- journal 位于 `<data-dir>/hub/publications.json`，每个实例受既有数据目录进程锁保护。
  发布内容、通知内容和 event ID 入盘后保持不可变，只有有效 `reply.result` 才确认完成。
- 中心 origin + node session 是 outbox 的授权范围。切换范围时丢弃旧范围的队列及目录版本；
  旧范围的迟到结果不能删除新范围的数据。显式停止时不发送，只有相同范围重新授权才可恢复。
- 临时失败采用 2–60 秒指数退避；永久参数、权限和冲突错误保留为隔离记录，不阻塞后续事件。
- 事件保留 24 小时，最多 1024 条；attention 去重键最多 4096 条，已确认记录仍参与去重。
  目录只保留各 kind 的最新快照；journal 编码最多 24 MiB，文件读取上限 32 MiB。
- post-commit 到 journal 之间是有界内存 ingress（256 条 / 4 MiB），不承诺与业务存储原子提交。
  正常停机刷新 ingress；重启、重连和变更后从 registry 补扫最近 24 小时的当前 Agent attention，
  更早的记录只进入目录状态，不重复生成提醒。
  ingress 溢出会记录错误并触发重建；尚未入盘的 terminal hook 事件在崩溃或溢出时无法恢复，
  Timeline 标记也需要客户端重新读取所属 Host 的持久历史。中心 journal 不是 Host 全量审计日志。

## 分阶段切换原生通知

事件和目录发布在中心声明能力后自动启用。独立 daemon 的原生投递默认继续使用旧 daemon 注册，
仅在设置 `AIT_SERVER_HUB_NOTIFICATIONS=true`（或 `1`）时接管中心原生通知；`false` / `0`
关闭该模式，其他值导致启动失败。设置应用于新入队的事件，已有 journal 事件保留原投递决策。

2026-10-26 修订：[ADR-182](../clients/adr-182-hub-account-client.md) 的桌面内置 daemon
默认传入开启值，并保留显式关闭选项；本 ADR 的独立 daemon 默认值不变。

开启前，客户端必须在 Hub access 链路实现 `push.register`、`session.heartbeat` 和事件订阅；
中心必须配置 APNs / FCM。仅部署新中心或新 daemon 不会迁移旧 token。
旧中心未声明方法时继续原路径。客户端接入、跨 Host 导航和真机推送验收由客户端 PR 完成。

## 验证

定向验证覆盖协议相关性、ACK / reply 区别、物理续传、新逻辑链路、容量与旧中心兼容；
发布队列覆盖重启、幂等、存储失败、范围切换、隔离、过期与快照合并；Host 生命周期测试
覆盖正常关闭与进程锁释放。Provider 自身的 Timeline 发布测试保留在 provider crate。

## 后果

Host 的业务执行和权威数据继续留在本机，账号分发与设备投递可以独立扩展；代价是 Host
需要维护有界 journal，并处理与中心部署及客户端切换的兼容。尚未进入 journal 的瞬时事件
仍依赖进程存活；如以后要求全量审计，需要在业务存储事务中引入统一的持久事件提交契约。
