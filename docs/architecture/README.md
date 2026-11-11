# 当前 Ait 架构

本页描述当前组件关系；决策背景和历史取舍见 [ADR 索引](../decisions/README.md)。
各能力当前生效的约束及其 ADR 见现行规格：[Provider](provider.md)、[Workspace](workspace.md)、
[Bot](bot.md)、[连接与传输](connection.md)。ADR 被接受后，结论并入对应规格。
Rust 服务入口是 `bins/daemon`，Cargo package 和可执行文件均为 `daemon`；桌面安装包与独立发布归档将它命名为 `aitd`。
独立外部 ACP 入口 `bins/omp-acp` 产出 `ait-omp-acp`；它只负责 CLI/stdio，协议转换和原生 RPC 归 provider，
见 [ADR-237](../decisions/providers/adr-237-omp-native-session-and-standalone-acp.md)。
Electron 位于 `apps/desktop`，`apps/mobile` 提供桌面、Web 和移动端共享界面；SDK 与协议在 `packages/`。

## Rust 能力边界

| Crate         | 职责                                          | 生产代码的内部依赖                                   |
| ------------- | --------------------------------------------- | ---------------------------------------------------- |
| `domain`      | 业务值、记录、不变量与纯投影                  | 无                                                   |
| `model`       | 协作端口、协议、请求资源与共享协调            | `domain`                                             |
| `persistence` | 原子文件、Registry、具体文件持久化适配器      | `model`、`domain`                                    |
| `metadata`    | 基础连接、Project/Workspace、配置与生命周期   | `domain`、`model`                                    |
| `filesystem`  | 文件、Git、Forge、worktree/clone 本地适配器   | `domain`、`model`                                    |
| `provider`    | 原生 Agent、执行、历史、摘要、技能与 MCP 注入 | `domain`、`model`                                    |
| `terminal`    | PTY、快照、活动与订阅                         | `domain`、`model`                                    |
| `voice`       | 语音、听写、离线推理与媒体连接                | `model`                                              |
| `bot`         | Workspace Bot、Channel、隐藏会话与消息协调    | `domain`、`model`、`provider`                        |
| `schedule`    | 定时任务服务与协议                            | `domain`、`model`                                    |
| `browser`     | 浏览器自动化与 TCP 隧道                       | `model`                                              |
| `relay`       | Hub serve、下载 Relay、控制与传输             | `model`                                              |
| `host-link`   | 机器授权、节点租约与发布协调                  | 无，通过 ports 调用适配器                            |
| `api`         | HTTP/WebSocket、鉴权、分发与跨能力协调        | `domain`、`model`、`host-link` 及上述功能 crate      |
| `daemon`      | CLI/配置、进程锁、身份、服务组装与停机        | `api`、`domain`、`model`、`persistence` 及功能 crate |
| `ait-omp-acp` | 外部 ACP CLI、stdio、诊断与退出               | `model`、`provider`                                  |

适配器实现所属能力的 port，应用服务协调领域行为。功能 crate 的持久化由 daemon 注入；
相关测试可以使用 `persistence`。`bot → provider` 是明确允许的单向协作边，Provider 不反向依赖 Bot。
`domain` 不含 Tokio、传输或存储实现；`model` 含异步请求资源，不属于纯领域层。
依赖守卫见 [`dependencies.rs`](../../bins/daemon/tests/dependencies.rs)，具体依赖以各 Cargo.toml 为准。

每个功能 crate 对外提供完整 `Service`，API 按 crate 安装，组件声明方法和消息方向。
`implemented_methods()` 与 `installed_methods()` 提供能力清单；metadata 声明基础连接方法。
请求校验后通过可消费的 `Option<Context>` 逐级分发；未匹配的入口必须保留请求。
见 [ADR-093](../decisions/daemon/adr-093-consumable-request-context.md)、
[ADR-100](../decisions/daemon/adr-100-crate-level-service-installation.md)。

## 共享数据与持久化

- 业务记录和纯算法归 `domain`；Workspace、会话、创建回执、目录同步、摘要和存储端口归 `model`。
  连接 envelope、协议版本、错误码和预算归 `model::server`，方法元数据归 `model::methods`。
- `persistence` 实现 Registry、配置、创建回执、push token、Agent runtime、Bot 和 schedule 的文件存储。
  功能服务负责流程，daemon 注入具体适配器；共享契约不执行文件 I/O。
- `metadata` 拥有 Project/Workspace 登记、setup、worktree/recovery/GitHub provisioning RPC；
  本地 Git、Forge 与目录操作由 `filesystem` 通过 model 端口提供。
- `provider` 拥有原生历史适配、Timeline SQLite 展示索引和 preset catalog；沙箱 Workspace 的写入
  confinement 由 agent manager 解析，`provider::local::sandbox` 负责生成平台命令（Linux bubblewrap 见 [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)，macOS Seatbelt 见 [ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)）；
  原生 Provider 的认证、工具和会话仍由各原生程序持有，不能合并为一个通用数据库入口。

依赖与数据归属见 [ADR-102](../decisions/providers/adr-102-provider-metadata-independence.md)、
[ADR-111](../decisions/daemon/adr-111-domain-values-and-server-protocol.md)、
[ADR-112](../decisions/daemon/adr-112-persistence-crate.md)、
[ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)。
metadata 和 filesystem 内部按能力组组织，组间只引用 ports/protocol；各自的 module boundary 测试约束这一点。

## Provider 与工作区生命周期

`provider::Providers` 组装 Codex、Claude Code、OpenCode、DeepSeek Harness、Antigravity、Pi、
Oh My Pi 与 MiniMax Code。可用性、模型和原生版本支持由运行时发现；操作说明见[文档索引](../README.md#agent-与工作区)。
每个稳定 Agent 身份的异步任务独占原生 session，不同身份独立执行；发现、已提交读取、完成等待
使用独立路径，阻塞存储操作离开专用 Tokio reactor。全局预算限制接纳和原生资源数量。
见 [ADR-091](../decisions/providers/adr-091-independent-session-execution.md)、
[ADR-205](../decisions/providers/adr-205-provider-execution-contracts.md)。

Agent registry 通过 `snapshot()` 与 `get_shared()` 提供已提交的不可变读视图；目录、
Workspace attention、生命周期检查和 Hub 摘要复用记录，写入成功后失效缓存。
旧视图保持有效，权限写入仍核对最新记录，见
[ADR-210](../decisions/providers/adr-210-committed-runtime-read-snapshots.md)。

Provider 拥有摘要生成、主机级技能安装与原生会话 MCP 注入。daemon 注入配置、资源和端点；
API 的 `/mcp/agents` 适配到已有 Rust RPC，不另建业务服务或存储。
Provider 自身行为测试归 `crates/provider`；daemon 测试覆盖宿主、传输和跨能力组合，见
[ADR-179](../decisions/providers/adr-179-provider-owned-tests.md)。

OMP 原生命令的来源投影、内置过滤、参数提示与补全接口台账见
[命令来源与补全](command-provenance.md)；决策与迁移表见
[ADR-242](../decisions/providers/adr-242-native-command-provenance-and-completion.md)（proposed，Draft PR 不构成架构评审批准）。

Pi 模型与推理偏好沿用配置持久化及下一条 prompt 前应用的流程；请求偏好与原生有效状态分别保存，
准入拒绝或 busy 时也在关闭 writer 前保存已知运行事实，见
[ADR-211](../decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)。

Pi 扩展 `notify` 由 provider 投影为持久化的会话通知；通知队列独立限额并让位于必要输出，
见 [ADR-220](../decisions/providers/adr-220-pi-extension-notifications.md)。

新定时任务保存固定 Workspace 身份，执行时校验归属，在该 Workspace 内选择或创建 Agent。
新建 Agent 的收尾不归档 Workspace；旧未绑定任务保留兼容分支，见[任务手册](../operations/schedules.md)。
Bot 持有独占 Channel 与隐藏 Provider session，普通 Agent API 不能接管；Workspace 归档使 Bot 退役（`retired`）并删除其记忆，见 [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)。
执行期间权限请求自动拒绝，重启后不重做结果不明的副作用输入，见[Bot 手册](../operations/bots.md)。
中心托管的 Webhook Channel 使用现有 serve 链路和受限服务会话，daemon 只保存端点；见 [ADR-233](../decisions/bot/adr-233-webhook-channel-center-endpoints.md)。

## 主机、连接与客户端

daemon 先预绑定 TCP 端口，再以 `instance.lock` 排他持有数据目录；锁覆盖本地令牌初始化、诊断采集和
运行时重启。`server-id` 稳定，`instance_id` 每个运行时变化。
启动配置、身份、故障证据及服务组装归 daemon。SIGINT/SIGTERM 与关闭 RPC 停止接纳、回收所属资源并释放锁。
本地 API token 自动持久化，显式 `AIT_SERVER_TOKEN` 优先；详细路径和恢复方式见[daemon 手册](../operations/daemon.md)。

所有 daemon 通过 `host-link` 自主管理机器授权、节点注册、续租和 Hub serve；daemon 不再建立 Relay control 或下载 Relay。
Desktop/移动客户端批准 device authorization 后把待领取授权交给 daemon；客户端只维护自身 access 节点。
退出客户端账号不撤销 daemon 授权；显式断开清除机器凭据，重启后也不会自动恢复发布。
见 [ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)。

| 通道                    | 当前用途                                                                          |
| ----------------------- | --------------------------------------------------------------------------------- |
| Hub access → Hub serve  | 在线业务 RPC、终端、实时语音、浏览器隧道；协商后承载租约、Host 列表、可见性和下载 |
| 中心 `/relay-downloads` | 客户端仅对未声明 `download` 的旧 Host 使用的下载入口                              |
| 直接 TCP / SSH          | 经认证的 daemon 直连，优先单连接                                                  |

`@ait/client` 原生处理 Rust v1 握手、请求关联、订阅归属、消息分块和二进制路由。
业务 API 直接传递对象给 SDK 内部协议模块，仅在物理传输处编码/解析 JSON。
`apps/mobile/src/runtime/daemon-transport` 选择平台连接，不维护独立的协议兼容层。
Electron 的 `rust-daemon.ts` 继续只负责本机进程管理。见
[ADR-211](../decisions/clients/adr-211-sdk-native-daemon-protocol.md)。

在线业务只使用 Hub，无传统 Relay/Yamux 回退，见
[ADR-204](../decisions/clients/adr-204-hub-only-online-business-transport.md)。
Hub feature 协商与旧版本兼容见
[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)。
Host 通过 `model::hub` 发布事件和目录摘要，`persistence` 保存发布 journal；中心方法由握手协商。
Desktop 内置 daemon 默认启用 Hub 原生通知，独立 daemon 默认直发，可用 `AIT_SERVER_HUB_NOTIFICATIONS` 覆盖。
通知配置和凭据归属见[移动端通知](../operations/mobile-notifications.md)。

macOS/Linux Desktop 管理自己启动或验证接管的 daemon，Windows Desktop 只连接远端。
就绪探测失败保留存活的自有进程供重试；退出时只交接已输出就绪日志的进程，未就绪进程被停止。
见 [ADR-207](../decisions/clients/adr-207-desktop-daemon-readiness-ownership.md)。
一个桌面主进程管理多个窗口，Workspace 以主机与工作区身份在新窗口直接打开。
窗口导航和当前布局独立，保存时按记录合并；聊天草稿在本地输入未变化时同步，附件由主进程聚合所有窗口引用后回收。
见 [ADR-223](../decisions/clients/adr-223-desktop-workspace-windows.md)。
桌面内置浏览器对本地 Workspace 直接联网，对远端 Workspace 的所有目标经对应 daemon 出站并解析 DNS，
见 [ADR-186](../decisions/clients/adr-186-workspace-browser-egress.md)。
浏览器直连先换取一次性 WebSocket 票据；连接所属订阅在关闭时释放。
Hub 会话支持物理链路续传，超出恢复窗口后仍需重新建立会话和订阅。

群组分享以 Host 保存的范围为准，Hub 访客通过 `x-ait-guest` 准入；workspace/project 限制访问范围，
node 分享允许更广的节点操作，分享和账号授权保留给所有者，Bot 与定时任务仅可读取。
创建访客 Agent／终端的任务在连接断开后仍完成归属记录与撤销检查；撤销逐页取消 Agent，
终端归属按存活状态清理。共享项目的配置文件和独立分享设置受保护，Forge 查询绑定到 checkout 远端。见
[ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)。

原生实时语音由 Provider 拥有原生 thread、SDP、上游 WebRTC、音频协议及认证，voice 拥有媒体连接
和 PCM 消费确认，daemon 桥接端口，客户端采集与播放，见
[ADR-191](../decisions/providers/adr-191-codex-native-realtime-voice.md)。

## 兼容名称

源码与产物使用 daemon 名称；`server.*`、`server_info`、`/v1/server/info`、`AIT_SERVER_*` 与已有数据目录
保留协议兼容拼写。旧 CLI、独立 worker 和旧 Project SQLite 架构已退役，不属于当前 workspace。
历史边界应按 ADR 的修订关系阅读；早期移植和已完成方案见[归档](../archive/README.md)。
