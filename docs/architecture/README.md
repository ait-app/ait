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
| `relay` | Outbound control and reverse data transport | None |
| `api`        | HTTP/WebSocket 鉴权、连接与跨能力协调      | 上述能力包、`protocol`、`model`        |
| `daemon`     | 配置、进程锁、服务组装和停机               | API、领域及能力包；测试使用 `protocol` |

能力包自己声明方法分组、安装条件与请求处理。API 组装具体服务，不把业务协议反向传入
能力包。具体 adapter 实现所属能力的 port；应用服务协调领域行为。`domain` 无 Tokio、
传输或存储依赖。`model` 含 Tokio 请求资源，不属于纯领域层。

依赖守卫位于 `bins/daemon/tests/dependencies.rs`。它检查 `cargo metadata --no-deps` 的全部
workspace 包与普通、开发、构建、optional 和平台条件依赖，拒绝未知内部包及向外依赖。

## 数据与进程所有权

daemon 持有数据目录的 OS 文件锁及稳定 `server-id`，同一目录只能有一个活动实例。
Project/Workspace 和 Agent runtime 目录、配置、时间线由当前文件存储与 Provider adapter
管理；Agent preset catalog 的 SQLite 存储位于 `provider`。这些职责不能合并成一个通用数据库入口。
原生 Codex、Claude Code 与 DeepSeek Harness 的凭据和会话仍由对应程序持有。

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

The desktop account manager owns user credentials and passes one-use grants to `api`.
The `api` crate owns `relay`, which receives a fixed local destination and has no
workspace dependencies. See [ADR-074](../decisions/clients/adr-074-account-host-relay.md).
