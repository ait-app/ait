# 当前 Ait 架构

Ait 的本机服务入口是 `bins/daemon`，Cargo package 和可执行文件均名为 `daemon`。
Electron 位于 `apps/desktop`；`apps/mobile` 提供桌面、浏览器与移动端共享界面，npm package
分别为 `@ait/desktop` 和 `@ait/mobile`。本地 SDK 和协议位于 `packages/`。

## Rust 能力边界

| Crate        | 职责                                         | 允许的内部依赖                         |
| ------------ | -------------------------------------------- | -------------------------------------- |
| `domain`     | Agent 身份与配置不变量                       | 无                                     |
| `file`       | 单文件工具、启动配置及具体文件持久化适配器 | `model`、`domain`                      |
| `model`      | 公共记录与契约、请求和事件资源、创建流程协调 | 无                                     |
| `protocol`   | WebSocket envelope、版本与能力协商           | `model`                                |
| `metadata`   | 基础连接方法、Project/Workspace 目录与配置 | `model`、`file`                        |
| `filesystem` | 文件、Git、worktree、Forge 和技能安装        | `model`；测试使用 `file`               |
| `provider`   | 原生 Provider 会话、执行、历史和摘要生成     | `domain`、`model`；测试使用 `file`      |
| `terminal`   | PTY、终端快照、活动和连接订阅                | `model`                                |
| `voice`      | 语音、听写和离线推理                         | `model`                                |
| `schedule`   | 定时任务服务与协议                           | `model`；测试使用 `file`               |
| `browser`    | 浏览器自动化请求与回传                       | `model`                                |
| `relay`      | 控制 RPC、主动连接与反向数据通道             | `model`                                |
| `api`        | HTTP/WebSocket 鉴权、连接与跨能力协调        | 上述能力包、`protocol`、`model`；测试使用 `file` |
| `daemon`     | 进程锁、服务组装和停机                       | API、领域、`file` 及能力包；测试使用 `protocol` |

各实现组件自己声明方法，功能 crate 对外提供一个完整的 `Service`，API 按 crate 整体安装。
`implemented_methods()` 返回业务方法声明，`installed_methods(bool)` 返回全部或空集合。
metadata 另由 `connection_methods()` 声明始终可用的九个基础连接方法，relay 声明三个控制
RPC；API 聚合这些声明，并复用 model 的会话和创建记录基础设施。内部功能对象
保留独立锁、适配器和任务，私有 transport composition 负责连接共享资源，不作为安装开关。
服务安装见 [ADR-100](../decisions/daemon/adr-100-crate-level-service-installation.md)，基础声明归属见
[ADR-108](../decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)。具体 adapter 实现所属能力的 port；应用服务协调领域行为。`domain` 无 Tokio、
传输或存储依赖。`model` 含 Tokio 请求资源，不属于纯领域层。

请求通过名称、方向和 capability 校验后，以 `Option<Context>` 逐级进入处理入口。每个入口
自行匹配，未匹配时保留请求，匹配后取走并执行；成功后 API 完成响应或跨能力收尾并返回，
仅在 `NotImplemented` 时断言 Context 仍为 `Some` 后继续。违约消费会先记录 error 再触发断言。
方法名称和消息方向由所属组件声明，功能 crate 聚合，API 汇总用于协议校验；业务处理
不预先选择 handler，详见 [ADR-093](../decisions/daemon/adr-093-consumable-request-context.md)
与 [ADR-095](../decisions/daemon/adr-095-component-method-declarations.md)。

摘要生成能力由 `provider::SummaryGenerator` 声明，输入输出类型位于 `model::summary`。
生成器通过自己的配置端口读取偏好，daemon 连接现有存储；API 将同一生成器适配为
model 的消费端口 `SummarySource`，见
[ADR-101](../decisions/providers/adr-101-provider-summary-generator.md)。

Provider 不依赖 metadata。共享 Workspace 记录、registry、活动与关注接口、worktree
契约和 wire 类型归 `model::workspace`；事件通道归 `model::session`，幂等创建协调归
`model::creation`，通过 `ReceiptStore` 使用持久化。通用原子文件引擎归 `file::registry`，
Project/Workspace registry 和标签事务归 `file::storage`，回执文件适配归 `file::creation`。
Workspace 登记、
setup 和命名仍由 metadata 实现，通过 `model::workspace::lifecycle` 的接口注入
provider。调用处直接导入共享定义，metadata 的旧共享转发路径已移除。
详见 [ADR-102](../decisions/providers/adr-102-provider-metadata-independence.md)。

`file` 提供单文件读取、原子写入和可取消的轮询观察，并承载通用 FileRegistry 的缓存、
提交锁、事务 hooks 和冻结。它通过 model/domain 契约实现 registry、创建回执、daemon/project
配置、project icon、push token、server identity、Agent runtime 和 schedule 文件持久化。
`model::storage` 只声明共享存储接口，不执行文件 I/O，也不依赖 file；功能服务保留流程协调，
daemon 注入具体适配器。daemon 启动 CLI/环境/TOML 由 `file::config` 加载，API 校验策略
仍由宿主注入。见 [ADR-106](../decisions/daemon/adr-106-concrete-file-persistence.md)，
修订前的通用工具提取见 [ADR-105](../decisions/daemon/adr-105-file-tools-and-startup-config.md)。

迁移时保留的纯 `pub use` 模块已删除；共享组件直接从 model/domain 导入，具体文件适配器
直接从 file 导入。provider 和 schedule 的生产代码仅通过存储契约接收宿主注入，测试使用
file 适配器；见 [ADR-107](../decisions/daemon/adr-107-direct-imports-from-owning-crates.md)。

Terminal 同样直接使用 model 的 Workspace registry、活动契约和连接事件资源，
不依赖 metadata；见 [ADR-103](../decisions/daemon/adr-103-terminal-model-dependency.md)。
Filesystem 的生产代码也只依赖 model，相关测试使用 file 适配器：共享目录观察、Git/Forge 快照、摘要消费、身份与 descriptor
纯函数归 model；Project 登记、命名和 setup 通过 `ProjectRegistration`、`WorkspaceNaming`
与 `WorkspaceSetup` 注入 metadata 的现有服务。API 的 setup 适配器继续使用原有自动化锁，
Runtime 保留阻塞执行、admission 和任务跟踪。功能 crate 之间没有直接依赖；见
[ADR-104](../decisions/workspace/adr-104-filesystem-model-collaboration.md)。

内置 Provider 由 `provider::Providers` 组装。具体客户端列表、启动配置、安装发现与辅助
元数据生成能力留在 provider crate 内部；daemon 提供数据目录并连接服务与进程生命周期。
Codex、Claude、OpenCode 和 DSH 当前支持结构化辅助生成，各 adapter 自行选择辅助小模型。
详见 [ADR-089](../decisions/providers/adr-089-provider-owned-composition.md) 与
[ADR-099](../decisions/providers/adr-099-provider-owned-auxiliary-models.md)。

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
`api` 持有 `relay`；`relay` 使用固定的本地目标地址，仅依赖 model 的共享 RPC 契约。
relay 拥有控制方法声明和处理，API 的 HTTP 路由与 WebSocket RPC 共用同一 connector，
鉴权、协商和停机由 API 管理。见 [ADR-108](../decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)，
中继产品流程见 [ADR-074](../decisions/clients/adr-074-account-host-relay.md)。

应用在线服务登录与主机发布分离。平台账户管理器维护显式选中 daemon 的独立租约，
客户端通过该主机的鉴权业务连接发送一次性控制票据，API 将控制请求交给 relay 执行。
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
