# 当前 Ait 架构

Ait 的本机服务入口是 `bins/daemon`，Cargo package 和可执行文件均名为 `daemon`。
Electron 位于 `apps/desktop`；`apps/mobile` 提供桌面、浏览器与移动端共享界面，npm package
分别为 `@ait/desktop` 和 `@ait/mobile`。本地 SDK 和协议位于 `packages/`。

## Rust 能力边界

| Crate        | 职责                                         | 允许的内部依赖                         |
| ------------ | -------------------------------------------- | -------------------------------------- |
| `domain`     | Agent 身份与配置不变量                       | 无                                     |
| `model`      | 公共契约、请求与事件资源、创建回执和文件存储 | 无                                     |
| `protocol`   | WebSocket envelope、版本与能力协商           | `model`                                |
| `metadata`   | Project/Workspace 目录、标签、配置和自动化   | `model`                                |
| `filesystem` | 文件、Git、worktree、Forge 和技能安装        | `metadata`、`model`                    |
| `provider`   | 原生 Provider 会话、执行、历史和摘要生成     | `domain`、`model`                      |
| `terminal`   | PTY、终端快照、活动和连接订阅                | `model`                                |
| `voice`      | 语音、听写和离线推理                         | `model`                                |
| `schedule`   | 定时任务服务与协议                           | `model`                                |
| `browser`    | 浏览器自动化请求与回传                       | `model`                                |
| `relay`      | 主动建立控制连接与反向数据通道               | 无                                     |
| `api`        | HTTP/WebSocket 鉴权、连接与跨能力协调        | 上述能力包、`protocol`、`model`        |
| `daemon`     | 配置、进程锁、服务组装和停机                 | API、领域及能力包；测试使用 `protocol` |

各实现组件自己声明方法，功能 crate 对外提供一个完整的 `Service`，API 按 crate 整体安装。
`implemented_methods()` 返回全部声明，`installed_methods(bool)` 返回全部或空集合；API
自行声明始终可用的连接方法，并复用 model 的会话和创建记录基础设施。内部功能对象
保留独立锁、适配器和任务，私有 transport composition 负责连接共享资源，不作为安装开关。
详见 [ADR-099](../decisions/daemon/adr-099-crate-level-service-installation.md)。具体 adapter 实现所属能力的 port；应用服务协调领域行为。`domain` 无 Tokio、
传输或存储依赖。`model` 含 Tokio 请求资源，不属于纯领域层。

请求通过名称、方向和 capability 校验后，以 `Option<Context>` 逐级进入处理入口。每个入口
自行匹配，未匹配时保留请求，匹配后取走并执行；成功后 API 完成响应或跨能力收尾并返回，
仅在 `NotImplemented` 时断言 Context 仍为 `Some` 后继续。违约消费会先记录 error 再触发断言。
方法名称和消息方向由所属组件声明，功能 crate 聚合，API 汇总用于协议校验；业务处理
不预先选择 handler，详见 [ADR-093](../decisions/daemon/adr-093-consumable-request-context.md)
与 [ADR-095](../decisions/daemon/adr-095-component-method-declarations.md)。

摘要生成能力由 `provider::SummaryGenerator` 声明，输入输出类型位于 `model::summary`。
生成器通过自己的配置端口读取偏好，daemon 连接现有存储；API 将同一生成器适配为
metadata 的消费端口 `SummarySource`，见
[ADR-100](../decisions/providers/adr-100-provider-summary-generator.md)。

Provider 不依赖 metadata。共享 Workspace 记录、registry、活动与关注接口、worktree
契约和 wire 类型归 `model::workspace`；事件通道归 `model::session`，幂等创建回执归
`model::creation`，原子文件 registry 和标签事务归 `model::storage`。Workspace 登记、
setup 和命名仍由 metadata 实现，通过 `model::workspace::lifecycle` 的接口注入
provider。metadata 的旧共享路径只重导出 model 类型，没有第二份状态或实现。
详见 [ADR-101](../decisions/providers/adr-101-provider-metadata-independence.md)。

Terminal 同样直接使用 model 的 Workspace registry、活动契约和连接事件资源，
不依赖 metadata；见 [ADR-102](../decisions/daemon/adr-102-terminal-model-dependency.md)。
功能 crate 之间剩余的直接依赖是 filesystem → metadata。

内置 Provider 由 `provider::Providers` 组装。具体客户端列表、启动配置、安装发现与辅助
元数据生成能力留在 provider crate 内部；daemon 提供数据目录并连接服务与进程生命周期。
Codex、Claude 当前支持结构化辅助生成，其他内置客户端同样属于原生 Provider。
详见 [ADR-089](../decisions/providers/adr-089-provider-owned-composition.md)。

依赖守卫位于 `bins/daemon/tests/dependencies.rs`。它检查 `cargo metadata --no-deps` 的全部
workspace 包与普通、开发、构建、optional 和平台条件依赖，拒绝未知内部包及向外依赖。

## 数据与进程所有权

daemon 持有数据目录的 OS 文件锁及稳定 `server-id`，同一目录只能有一个活动实例。
Project/Workspace 和 Agent runtime 目录、配置、时间线由当前文件存储与 Provider adapter
管理；Agent preset catalog 的 SQLite 存储位于 `provider`。这些职责不能合并成一个通用数据库入口。
原生 Codex、Claude Code、Antigravity、OpenCode 与 DeepSeek Harness 的凭据和会话仍由对应程序持有。

桌面进程只管理自己启动的 daemon 子进程，生成连接凭据并通过主进程 bridge 提供授权。
浏览器用一次性 WebSocket 票据连接；桌面、Web 和移动端使用同一个 Rust transport adapter。
连接关闭释放所属订阅，daemon 负责已接纳任务与原生进程的生命周期。

Provider 的原生 session 由稳定身份所属的异步任务独占，不同身份独立执行和提交事件。
Catalog discovery、已提交读取与 completion watch 使用独立路径；阻塞存储操作离开专用
Tokio reactor，级联归档与 worktree 清理用相关会话屏障协调，全局预算限制接纳和 native
资源总量。详见 [ADR-091](../decisions/providers/adr-091-independent-session-execution.md)。

## 名称与兼容

源码、构建产物和运行日志使用 daemon 名称。WebSocket `server_info`、`server.*` 方法、
`/v1/server/info`、稳定身份文件、`AIT_SERVER_*` 配置与已有数据目录保持兼容。
当前客户端 API 使用这些字段；目录重命名不迁移或删除用户数据。

旧 CLI、旧独立 worker 及其 Project SQLite 架构已不在当前 workspace 中。旧实现的资料
从文档树移除，历史可从 Git 查阅。当前语义依据 [ADR 分类索引](../decisions/README.md)，
启动与连接依据 [daemon 手册](../operations/daemon.md)。

桌面账户管理器持有用户凭据，并向 `api` 传递一次性授权。
`api` 持有 `relay`；`relay` 使用固定的本地目标地址，不依赖其他 workspace crate。
详见 [ADR-074](../decisions/clients/adr-074-account-host-relay.md)。

应用在线服务登录与主机发布分离。平台账户管理器维护显式选中 daemon 的独立租约，
客户端通过该主机的鉴权业务连接发送一次性控制票据，`api` 管理 `relay` 的状态、启动与停止。
客户端退出只释放自身节点及绑定 daemon，其他 daemon 保留各自原账户授权，由运行中的平台账户管理器继续续租；主动停止同步只撤销单台租约。
本机、TCP 和 SSH 主机使用相同同步流程，详见 [ADR-083](../decisions/clients/adr-083-online-service-host-sync.md)。Desktop 登录后默认同步内置 daemon，手动停止的选择由桌面账户存储持久保存；其他主机仍需手动启用，详见 [ADR-096](../decisions/clients/adr-096-desktop-default-host-sync.md)。

账户会话状态机位于 `packages/client`，通过依赖注入获取平台身份、存储、HTTP 和运行时操作。
Electron 主进程提供桌面适配；Android 的原生适配使用 SecureStore 保存账户令牌，注册无本地
运行时的客户端节点。Android 通过带认证头的原生 WebSocket 建立中继连接与下载，只有选中的
远程主机进入 HostRuntime。浏览器和 iOS 未启用账户入口。
前后台生命周期、配对校验与凭据边界见
[ADR-076](../decisions/clients/adr-076-android-account-relay.md)。

Relay 的类型化消息集中在 `crates/relay/src/protocol.rs`，WebSocket 收发集中在
`transport.rs`；单连接协商标识由 `crates/protocol/src/single.rs` 定义。
模块职责见 [ADR-075](../decisions/clients/adr-075-relay-protocol-modules.md)。
