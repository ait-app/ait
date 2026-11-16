# daemon：启动、配置与连接

`aitd` 是 Ait 唯一的 Rust 服务入口。它负责 Project/Workspace、原生 Provider 会话、
Git/文件、终端、语音、调度和浏览器能力。源码边界见 [当前架构](../architecture/README.md)。
标准 MCP 客户端通过 `/mcp/agents` 使用编排工具，配置和用法见 [MCP 手册](mcp.md)。

## 启动与配置

```sh
cargo run -p daemon --bin daemon -- --listen 127.0.0.1:7316
```

所有启动方式优先从 `AIT_SERVER_TOKEN` 读取凭据，要求 32–256 字节可见 ASCII、无空格。
未配置时自动生成并复用 `<data-dir>/local-api.token`（0600）；Desktop 使用同一个 token。
不从命令行、URL、TOML 或 `.env` 自动加载服务凭据。

| 参数                   | 环境变量                | 默认值                                 |
| ---------------------- | ----------------------- | -------------------------------------- |
| `--data-dir`           | `AIT_SERVER_DATA_DIR`   | `~/.ait`，见下文旧目录查找顺序         |
| `--listen`             | `AIT_SERVER_LISTEN`     | `127.0.0.1:7316`                       |
| `--mcp-socket`         | `AIT_SERVER_MCP_SOCKET` | 不监听 Unix socket                     |
| `--config`             | 无                      | `<data-dir>/config.toml`，不存在则跳过 |
| `--log-level`          | `AIT_SERVER_LOG_LEVEL`  | `info`                                 |
| `--web-origin`，可重复 | 无                      | 无额外浏览器来源                       |

启动配置优先级为命令行、环境变量、TOML、默认值。目录不接受 TOML 覆盖。
显式 `--config` 指定的文件必须存在，未知字段和无效内容会拒绝启动：

```toml
listen = "127.0.0.1:7316"
# macOS / Linux 可选；路径须为绝对路径，父目录须存在。
# mcp_socket = "/absolute/private-directory/mcp.sock"
log_level = "info"
web_origins = ["http://localhost:8081", "http://127.0.0.1:8081"]
```

支持 IPv4、IPv6、具体网络地址和通配监听；端口 `0` 用于隔离测试。通配监听的客户端使用
具体地址连接。桌面 Host 设置保存监听 IP/端口，环境变量可覆盖设置。
浏览器页面来源须显式配置 HTTP loopback origin，包含端口且不带路径。

`--mcp-socket` 在现有 TCP 监听之外增加 MCP 专用 Unix socket，使用 `0600` 权限及相同 Bearer
鉴权。socket 上只暴露 `/mcp/agents`，连接示例和清理规则见 [MCP 手册](mcp.md#unix-socket-监听)。

`AIT_SERVER_*` 是兼容名称。未显式指定数据目录时，独立 daemon 与桌面依次使用已存在的
`~/.ait`、`~/.ait-server-desktop`、`~/.ait-server`；都不存在时创建 `~/.ait`，旧目录不会被移动
（[ADR-216](../decisions/clients/adr-216-default-data-directory-ait.md)）。开发桌面默认使用 `.tmp/ait/server`，开发 Web 默认使用 `.tmp/app/server`。
`AITD_BIN` 可指定已构建的 daemon；新默认构建产物为 `target/debug/daemon`。

桌面启动内置 daemon 时按白名单透传 Provider 启动环境：八个 `AIT_SERVER_*_BIN`
（`CODEX`、`CLAUDE`、`ANTIGRAVITY`、`OPENCODE`、`DEEPSEEK_HARNESS`、`PI`、`OMP`、`MINIMAX_CODE`）
以及 `AIT_SERVER_DEEPSEEK_HARNESS_PROFILE`、`AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT`，取值原样保留。
其余 `AIT_SERVER_*`（监听、数据目录、日志、token 等）仍被剥离，由桌面显式注入自身配置；
非 `AIT_SERVER_` 前缀的环境不受影响。独立启动的 daemon 直接读取进程环境，不经过该白名单。

### Hub 事件与中心原生通知

发布 Hub serve 后，daemon 根据 `welcome.center_methods` 自动上报已提交事件及
project / workspace / agent 轻量目录。重试记录保存在数据目录的 `hub/publications.json`，
与认证的中心地址及 node session 隔离。详细边界见
[ADR-181](../decisions/daemon/adr-181-hub-host-publication.md)。

客户端完成 Hub access 的设备注册、前台心跳和事件订阅，且中心已配置 APNs / FCM 后，
设置 `AIT_SERVER_HUB_NOTIFICATIONS=true`（或 `1`）并重启 daemon，将新事件的原生通知
交给中心。独立启动默认值为 false，继续使用在 daemon 上注册的 token；Desktop 启动内置 daemon 时默认设为 true。`false` / `0`
显式关闭，其他值拒绝启动。已有 journal 事件保留入队时的投递决策。

## 身份、数据与关闭

同一数据目录由 `instance.lock` 的 OS 文件锁排他管理。`server-id` 是原子保存的稳定身份，
`instance_id` 每次启动变化；身份损坏会拒绝启动。Unix 新建目录使用 0700 权限。

当前 Project/Workspace 位于 `projects/`，Agent runtime 与时间线位于 `agents/`，Agent preset
catalog 使用 `catalog.sqlite3`。具体数据归属由能力包维护，不把目录记录和原生 Provider
历史视为同一个存储。旧实现的 Project 数据格式不由当前 daemon 自动转换。

SIGINT、SIGTERM 和关闭 RPC 会停止接纳新连接、释放订阅、回收所属 Provider/终端资源，
再释放实例锁。重启 RPC 在同一进程中重新组装服务。桌面只停止自己创建的 daemon 子进程。
开启 **Host → Daemon → Keep daemon running after quit** 后，退出时 daemon 继续运行，连接信息写入
数据目录的 `desktop-daemon.json`（0600，含访问 token）；下次启动经认证与 PID 校验后重新接管，
见 [ADR-128](../decisions/clients/adr-128-desktop-daemon-keep-running.md)。
日志使用 `daemon ready listen=...` 表示监听就绪；客户端仍需通过认证握手确认服务可用。
桌面就绪探测超时或断线不会关闭仍存活的 daemon；重试启动会连接同一进程，显式停止或重启仍有效。
跨桌面会话接管失败时保留交接记录供重试，不启动竞争进程，见 [ADR-207](../decisions/clients/adr-207-desktop-daemon-readiness-ownership.md)。

## HTTP 与浏览器认证

| Endpoint                  | 认证                       | 行为                             |
| ------------------------- | -------------------------- | -------------------------------- |
| `GET /healthz`            | 无                         | 存活检查                         |
| `GET /readyz`             | 无                         | ready 时 200，draining 时 503    |
| `GET /v1/server/info`     | Bearer                     | 身份、监听地址、版本、能力和预算 |
| `GET /v1/ws`              | Bearer，或浏览器一次性票据 | WebSocket 升级                   |
| `POST /v1/auth/ws-ticket` | Bearer 与已允许的 Origin   | 浏览器 WebSocket 一次性票据      |

```sh
curl http://127.0.0.1:7316/readyz
curl -H "Authorization: Bearer $AIT_SERVER_TOKEN" http://127.0.0.1:7316/v1/server/info
```

路由检查 Host 和 Origin，拒绝重复认证头。浏览器 transport 先交换短时一次性票据，
再通过 WebSocket 子协议发送票据；服务 Bearer 不放入 URL。文件下载使用文件接口单独
签发的一次性 token。桌面通过主进程 transport bridge 提供认证。

独立 Web 开发入口：

```sh
npm run dev:mobile
```

脚本构建共享 UI 依赖和 daemon，启动 Expo Web，并配置两个本机页面来源。
连接表单填写实际服务地址和 token。更多说明见 [移动端与 Web](../../apps/mobile/README.md)。

## WebSocket 与能力

功能 crate 通过一个完整服务安装，`implemented_capabilities` 按已安装 crate 汇总其全部方法。
基础连接与 relay 方法始终可用；业务后端的安装、认证和模型状态由调用结果与诊断表达。
组装边界见 [ADR-100](../decisions/daemon/adr-100-crate-level-service-installation.md)。

客户端首先发送 hello，协商协议与所需方法：

```json
{
  "type": "hello",
  "client_id": "local-client",
  "protocol": { "major": 1, "min_minor": 0, "max_minor": 0 },
  "capabilities": ["server.info", "connection.ping"],
  "required_capabilities": []
}
```

响应 `type=server_info` 包含连接身份、协商结果和服务信息。`info.capabilities` 表示可协商
方法；`info.implemented_capabilities` 表示已安装处理器；行为版本由 feature 标记区分。
不要用历史报告中的数量判断当前能力。未知 required capability 拒绝握手；未协商的方法
不能调用。已登记占位方法返回 `not_implemented`，未知方法返回 `method_not_found`。

```json
{ "type": "request", "request_id": "r1", "method": "connection.ping", "params": { "nonce": "n1" } }
```

```json
{ "type": "response", "request_id": "r1", "result": { "nonce": "n1" } }
```

订阅归物理连接所有，断开时释放。`subscription.release.request` 幂等释放本连接订阅；
重连需重新订阅。输入幂等键由具体创建/发送方法定义，`request_id` 只关联响应。
终端使用对应 binary frame，浏览器回传使用对应 response envelope，不能替换为普通 RPC。

公开的 `server.*`、`server_info` 和 HTTP 路径保留协议拼写。
方法名称和消息方向由各 Rust 组件声明，通过功能 crate 的 `capabilities::implemented_methods`
汇总；公共元数据类型见 `crates/model/src/methods.rs`。SDK 与适配器见
[客户端说明](../../packages/client/README.md)和 [协议说明](../../packages/protocol/README.md)。

## Provider 与扩展能力

- Codex 使用本机 `codex app-server`；`AIT_SERVER_CODEX_BIN` 覆盖程序路径。
- [Claude Code](claude-code.md)使用本机认证与原生会话；`AIT_SERVER_CLAUDE_BIN` 覆盖路径。
- [DeepSeek Harness](deepseek-harness.md)默认使用原生 Web Host；设置 `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 时改用 stdio ACP。
- OpenCode 使用官方 `opencode acp`；`AIT_SERVER_OPENCODE_BIN` 覆盖程序路径，见
  [ADR-115](../decisions/providers/adr-115-opencode-acp-provider.md)。
- [Antigravity CLI](antigravity.md)使用官方或 Homebrew 安装的 AGY；`AIT_SERVER_ANTIGRAVITY_BIN` 覆盖路径。
- [Pi](pi.md)使用本机 `pi --mode rpc`；`AIT_SERVER_PI_BIN` 覆盖程序路径。
- [Oh My Pi](oh-my-pi.md)每个 Agent 运行一个官方 `omp --mode rpc-ui` 进程，直接使用原生 RPC；`AIT_SERVER_OMP_BIN` 覆盖程序路径。
- [MiniMax Code](minimax-code.md)使用官方 `mcode acp`；`AIT_SERVER_MINIMAX_CODE_BIN` 覆盖程序路径。
- 上述 `AIT_SERVER_*_BIN` 与 DeepSeek Harness 的 `PROFILE`/`TRANSPORT` 覆盖由桌面按白名单
  透传给内置 daemon（见[启动与配置](#启动与配置)）；其余 `AIT_SERVER_*` 环境不会进入内置 daemon。
- [语音与听写](speech.md)默认使用离线模型，也支持文档列出的远端后端。
- GitHub/Forge 操作使用当前机器上的 Git、`gh` 或 `glab`。

原生 Provider 凭据由对应程序保存。Ait 连接 token 与 Provider 凭据用途不同。
更多行为边界见 [分类 ADR](../decisions/README.md)，验证范围见 [报告索引](../archive/reports/README.md)。

Linux 无人值守运行支持 `aitd login` 和 `aitd run`，
接入条件与命令见 [无人值守 daemon](headless-daemon.md)。

## 用量账本

每台 Host 在 `<data_dir>/agents/usage.sqlite3` 记录它发起的 Provider（harness）调用，以及可获得的请求级 token 用量，
设计见 [ADR-246](../decisions/providers/adr-246-harness-usage-ledger.md)，规格见 [Provider 规格](../architecture/provider.md#harness-用量账本)。

- **启动与关闭**：账本在启动时打开，把上次运行遗留的 `running` 调用标记为 `interrupted`；关闭时写完队列再退出。
  文件无法打开时 daemon 记录 error 日志并继续运行但不记录，此时 `provider.usage.ledger.request` 返回 `unsupported_capability`。
  账本写入失败只记日志，不影响或延迟轮次。
- **保留期**：明细行（`calls`、`requests`）保留 90 天，`daily_usage` 日汇总（UTC 日）无限期保留。
  按 `outcome` 分组只读明细行，因此只覆盖 90 天窗口；`missing_calls` 等其他计数来自日汇总。
- **隐私**：不记录提示词或输出文本、文件路径和凭据；数据留在本机，不进入 Hub outbox，也不上传。
- **可信度标记**：调用的 `usage_status` 为 `exact`（每个请求都是 `request` 边界且完整）、`lower_bound`（可能漏记）或
  `missing`（无请求或 Provider 不上报）；请求的 `boundary` 为 `request`、`turn` 或 `cumulative_delta`。
  失败、取消或被中断的调用可能丢失进行中的请求，因此永远不是 `exact`：有请求时为 `lower_bound`，没有时为 `missing`。
  应用对含 `lower_bound` 或 `missing` 调用的数值显示“≥”。

### 各适配器的精度

| Provider                | 边界               | 说明                                                                                                            |
| ----------------------- | ------------------ | --------------------------------------------------------------------------------------------------------------- |
| Claude                  | `request`          | 含子 Agent 帧（有则计入）                                                                                       |
| Codex                   | `request`          | `input` 已扣除缓存读写；`cache_write` 取 `cacheWriteInputTokens`，缺失时为 0；子 Agent 线程在父轮次运行期间计入 |
| Pi、OMP                 | `request`          |                                                                                                                 |
| MiniMax                 | `turn`             | 整轮合计                                                                                                        |
| Antigravity             | `cumulative_delta` | 累计值求差；进程重启后的第一轮只作基线，该调用显示为 `missing`                                                  |
| OpenCode、ACP、DeepSeek | `missing`          | 只有调用次数                                                                                                    |
| 摘要与标题调用          | `missing`          | 只有调用次数                                                                                                    |

### 已知限制

- Claude 在轮次结束后才到达的后台子 Agent 帧不计入；Codex 子 Agent 线程在父轮次结束后才上报的用量同样不计入。
- OMP 子 Agent 的 token 不计入。
- 心跳提示沿用该 Agent 自己的来源（通常为 `user`）；定时任务创建的 Agent 来源为 `schedule`，按标签查找。
- MiniMax 与 Antigravity 的缓存语义按其上报原样映射，缓存可能与 `input` 重叠。
- Pi 的一个分段在接近显示事件上限时可能被丢弃，并记录警告。

### 查询与客户端

`provider.usage.ledger.request` 按 `from`（含）和 `to`（不含，毫秒）汇总，范围最长 400 天，
`groupBy` 取 `provider`、`model`、`source`、`outcome` 或 `day`。客户端读取 `features.providerUsageLedger`
协商；SDK 为 `DaemonClient.listUsageLedger`。应用的 Usage 对话框 → History 标签页显示 24 小时 / 7 天 / 30 天，
汇总所有 Host，不可达的 Host 单独列出；Host 设置页显示 30 天合计。

## 统一发布与本地 API 凭据

直接启动、`aitd run` 和 Desktop 内置 daemon 使用同一发布流程：daemon 持有机器授权，
自行注册节点、续租与重连。未登录时仍提供本地 API。在线业务只使用 Hub，下载使用独立 Relay。

本地 API token 的生成与覆盖规则见[启动与配置](#启动与配置)。机器授权的登录、运行中交接、
状态查询、完整断开和恢复统一见[无人值守 daemon](headless-daemon.md#运行中的授权与统一断开)。
显式断开会清除机器授权，重启不自动上线；本地 API token、任务和工作区数据保留。
