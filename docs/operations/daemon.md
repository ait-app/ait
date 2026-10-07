# daemon：启动、配置与连接

`daemon` 是 Ait 唯一的 Rust 服务入口。它负责 Project/Workspace、原生 Provider 会话、
Git/文件、终端、语音、调度和浏览器能力。源码边界见 [当前架构](../architecture/README.md)。

## 启动与配置

```sh
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
cargo run -p daemon --bin daemon -- --listen 127.0.0.1:7316
```

凭据只从 `AIT_SERVER_TOKEN` 读取，要求 32–256 字节可见 ASCII、无空格。
不从命令行、URL、TOML 或 `.env` 自动加载服务凭据。

| 参数                   | 环境变量               | 默认值                                 |
| ---------------------- | ---------------------- | -------------------------------------- |
| `--data-dir`           | `AIT_SERVER_DATA_DIR`  | `~/.ait-server`                        |
| `--listen`             | `AIT_SERVER_LISTEN`    | `127.0.0.1:7316`                       |
| `--config`             | 无                     | `<data-dir>/config.toml`，不存在则跳过 |
| `--log-level`          | `AIT_SERVER_LOG_LEVEL` | `info`                                 |
| `--web-origin`，可重复 | 无                     | 无额外浏览器来源                       |

启动配置优先级为命令行、环境变量、TOML、默认值。目录不接受 TOML 覆盖。
显式 `--config` 指定的文件必须存在，未知字段和无效内容会拒绝启动：

```toml
listen = "127.0.0.1:7316"
log_level = "info"
web_origins = ["http://localhost:8081", "http://127.0.0.1:8081"]
```

支持 IPv4、IPv6、具体网络地址和通配监听；端口 `0` 用于隔离测试。通配监听的客户端使用
具体地址连接。桌面 Host 设置保存监听 IP/端口，环境变量可覆盖设置。
浏览器页面来源须显式配置 HTTP loopback origin，包含端口且不带路径。

`AIT_SERVER_*` 和已有数据目录是兼容名称。桌面默认数据位于 `~/.ait-server-desktop`；
开发桌面默认使用 `.tmp/ait/server`，开发 Web 默认使用 `.tmp/app/server`。
`AIT_SERVER_BIN` 可指定已构建的 daemon；新默认构建产物为 `target/debug/daemon`。

## 身份、数据与关闭

同一数据目录由 `instance.lock` 的 OS 文件锁排他管理。`server-id` 是原子保存的稳定身份，
`instance_id` 每次启动变化；身份损坏会拒绝启动。Unix 新建目录使用 0700 权限。

当前 Project/Workspace 位于 `projects/`，Agent runtime 与时间线位于 `agents/`，Agent preset
catalog 使用 `catalog.sqlite3`。具体数据归属由能力包维护，不把目录记录和原生 Provider
历史视为同一个存储。旧实现的 Project 数据格式不由当前 daemon 自动转换。

SIGINT、SIGTERM 和关闭 RPC 会停止接纳新连接、释放订阅、回收所属 Provider/终端资源，
再释放实例锁。重启 RPC 在同一进程中重新组装服务。桌面只停止自己创建的 daemon 子进程。
日志使用 `daemon ready listen=...` 表示监听就绪；客户端仍需通过认证握手确认服务可用。

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
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
npm run dev:mobile
```

脚本构建共享 UI 依赖和 daemon，启动 Expo Web，并配置两个本机页面来源。
连接表单填写实际服务地址和 token。更多说明见 [移动端与 Web](../../apps/mobile/README.md)。

## WebSocket 与能力

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
- [DeepSeek Harness](deepseek-harness.md)使用 stdio ACP 与动态模型目录。
- [语音与听写](speech.md)默认使用离线模型，也支持文档列出的远端后端。
- GitHub/Forge 操作使用当前机器上的 Git、`gh` 或 `glab`。

原生 Provider 凭据由对应程序保存。Ait 连接 token 与 Provider 凭据用途不同。
更多行为边界见 [分类 ADR](../decisions/README.md)，验证范围见 [报告索引](../reports/README.md)。
