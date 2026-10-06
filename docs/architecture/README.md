# 当前 Ait 架构

Ait 的本机服务入口是 `bins/daemon`，Cargo package 和可执行文件均名为 `daemon`。
Electron 位于 `apps/desktop`；`apps/mobile` 提供桌面、浏览器与移动端共享界面，npm package
分别为 `@ait/desktop` 和 `@ait/mobile`。本地 SDK 和协议位于 `packages/`。

## Rust 能力边界

| Crate        | 职责                                       | 允许的内部依赖                         |
| ------------ | ------------------------------------------ | -------------------------------------- |
| `domain`     | Agent 身份与配置不变量                     | 无                                     |
| `model`      | 公共错误、消息、请求上下文和运行资源       | 无                                     |
| `protocol`   | WebSocket envelope、能力协商和静态方法目录 | `model`                                |
| `metadata`   | Project/Workspace 目录、标签、配置和自动化 | `model`                                |
| `filesystem` | 文件、Git、worktree、Forge 和技能安装      | `metadata`、`model`                    |
| `provider`   | 原生 Provider 会话、执行、历史和元数据生成 | `domain`、`metadata`、`model`          |
| `terminal`   | PTY、终端快照、活动和连接订阅              | `metadata`、`model`                    |
| `voice`      | 语音、听写和离线推理                       | `model`                                |
| `schedule`   | 定时任务服务与协议                         | `model`                                |
| `browser`    | 浏览器自动化请求与回传                     | `model`                                |
| `relay`      | 主动建立控制连接与反向数据通道             | 无                                     |
| `api`        | HTTP/WebSocket 鉴权、连接与跨能力协调      | 上述能力包、`protocol`、`model`        |
| `daemon`     | 配置、进程锁、服务组装和停机               | API、领域及能力包；测试使用 `protocol` |

能力包自己声明方法分组、安装条件与请求处理。API 组装具体服务，不把业务协议反向传入
能力包。具体 adapter 实现所属能力的 port；应用服务协调领域行为。`domain` 无 Tokio、
传输或存储依赖。`model` 含 Tokio 请求资源，不属于纯领域层。

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
本机、TCP 和 SSH 主机使用相同入口，详见 [ADR-083](../decisions/clients/adr-083-online-service-host-sync.md)。

账户会话状态机位于 `packages/client`，通过依赖注入获取平台身份、存储、HTTP 和运行时操作。
Electron 主进程提供桌面适配；Android 的原生适配使用 SecureStore 保存账户令牌，注册无本地
运行时的客户端节点。Android 通过带认证头的原生 WebSocket 建立中继连接与下载，只有选中的
远程主机进入 HostRuntime。浏览器和 iOS 未启用账户入口。
前后台生命周期、配对校验与凭据边界见
[ADR-076](../decisions/clients/adr-076-android-account-relay.md)。

Relay 的类型化消息集中在 `crates/relay/src/protocol.rs`，WebSocket 收发集中在
`transport.rs`；单连接协商标识由 `crates/protocol/src/single.rs` 定义。
模块职责见 [ADR-075](../decisions/clients/adr-075-relay-protocol-modules.md)。
