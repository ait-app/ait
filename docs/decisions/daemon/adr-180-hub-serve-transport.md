# ADR-180：daemon 仅通过消息 Hub 提供远程接入

- 状态：已实现（Host serve；客户端 access 迁移另行实施）
- 日期：2026-10-22
- 关系：替代 ADR-074、ADR-075、ADR-108、ADR-155 中 daemon 的旧 Relay 传输和控制契约。账户和机器授权、节点租约继续沿用。

## 背景与决策

中心 `ait-server` 的 `ait-hub-v1` 已提供按账号隔离的 access/serve 链路、会话路由和 120 秒断线恢复。按用户要求直接删除 daemon 的旧 Relay，实现单一 Hub 传输，不增加并行兼容通道。

- 删除 `crates/relay`、旧控制/反向数据 WSS、Yamux 和旧独立下载桥；新增 `crates/hub`，只依赖 `model` 的 RPC 契约。
- `api` 持有唯一 Hub connector，本地入口为经过原有鉴权的 `GET/PUT/DELETE /api/hub/control`；业务连接上的控制方法为 `hub.status.request`、`hub.start.request`、`hub.stop.request`。旧入口和旧方法不提供别名。
- 桌面 Host 发布与 headless 均向 `POST /v1/node-sessions/{id}/hub-tickets` 请求 `{"ep":"serve"}`。长期用户或机器凭据留在账户管理器；connector 只接收一次性 Hub 票据和固定的本机 API 凭据。
- 节点注册暂时沿用中心已有的 `runtime.relay_modes: ["ait-rust-single-v1"]` 字段：当前中心仍强制校验该值。该字段仅用于节点租约注册，daemon 不再实现或连接该旧传输；移除该注册字段需要先修改中心的注册契约。
- Headless 的传输 port 改名为 `ManagedHub`；续租、轮换、退出和停机仍由 `host-link` 协调。被替换的 serve 链路不自动抢占回来，授权撤销等待现有租约/认证管理处理。
- Hub 二进制 envelope 为大端 u16 header 长度、最多 4 KiB JSON header 和最多 1 MiB 原始 payload。业务 hello 放在 open payload；Host 连接固定的本机 `/v1/ws`，转发原始 hello，收到 `server_info` 后才发 accept。中心不能选择本地 URL 或获取本地 token。
- 一条物理 WSS 承载最多 16 个会话，各会话有独立的本机 WebSocket。队列按消息数和字节数约束；慢会话或无效业务 hello 只关闭相应会话。
- 物理断线保留本机连接、订阅、接收水位和未确认帧；使用仅存内存的 resume token 恢复同一 link。重发沿用原序号，按序号去重。中心可能合并 presence 或丢弃被关闭会话的数据，因此接收序号允许空洞。
- Hub ACK 表示帧已被该传输接纳，不等于业务执行成功。新 link 不承接旧请求和旧序号。授权错误、链路替换、非法协议、恢复过期和 daemon 停机关闭本机会话；新票据必须由授权所有者重新申请。
- 重放缓冲、入站投递预算、出站投递预算分别限制为 8 MiB，消息数也有上限；关闭帧预留有限空间，避免数据满额时无法关闭单个会话。这些是应用缓冲预算，不是进程 RSS 上限。网络写入和本机投递均有截止时间。
- 本阶段不定义 Hub 下载会话、下载流控或广播业务内容。保留直接访问使用的本机 HTTP 文件下载功能。客户端 access、presence 投影及下载迁移完成前，旧客户端不能通过其旧 Relay 路径访问新 daemon。

## 后果与验证

中心只需按已有 Hub 契约路由消息；新 daemon 不再请求旧 control ticket，也不再连接旧 Relay 端点。桌面提供本机 daemon 时，界面 access 与 daemon serve 仍是不同端点；此处只完成 serve。

定向测试覆盖二进制 framing、序号/ACK、重放、会话隔离与容量、恢复过期、替换、HTTP/RPC 鉴权和真实本机 API 的 hello/ping 转发。模拟中心使用真实回环 WebSocket，不访问生产账号；未部署，未使用中心 PostgreSQL 集成环境。

## Test coverage

**not measured**：用户未要求覆盖率，本次仅运行改动及直接相关的定向测试；没有可比覆盖率基线或覆盖率产物。测试通过数不代表覆盖率。

验证基线为 `54de25d2` 加本次工作树。命令及范围：

- `RUSTC_WRAPPER= cargo test -p hub --offline`：Hub crate，包括回环传输 fixture。
- `RUSTC_WRAPPER= cargo test -p api --lib hub_rpc --offline`：本地控制入口与 Hub 到真实 API 转发。
- `RUSTC_WRAPPER= cargo test -p api --lib capabilities --offline` 和 `connection::dispatch` 过滤测试：能力目录、握手和分发。
- `RUSTC_WRAPPER= cargo test -p host-link --lib --offline`：授权和 Host 生命周期。
- `RUSTC_WRAPPER= cargo test -p daemon --bin daemon device::http --offline`：Hub serve 票据的 HTTP adapter。
- `RUSTC_WRAPPER= cargo test -p daemon --bin daemon headless --offline`：headless 接入生命周期。
- `RUSTC_WRAPPER= cargo test -p daemon --test dependencies --offline`：新 crate 的依赖约束。
- `RUSTC_WRAPPER= cargo test -p daemon --test process production_installs_every_in_scope_method_without_placeholders --offline`：实际 daemon 方法目录。
- `cargo fmt --all --check`、受影响 crate 的 Clippy `-D warnings`、workspace 编译检查、SDK 构建、Host 发布相关 TypeScript 定向测试、类型检查、改动文件格式/lint 和文档链接检查。

未运行完整 Rust workspace 测试。回环网络测试需要在允许监听本机端口的环境运行；本机 sccache 在沙箱内不可用，因此验证命令清空 `RUSTC_WRAPPER`。

移动端完整类型检查受当前环境缺失的 Markdown 依赖和未修改的 `@gorhom/bottom-sheet` 导出错误阻塞。移动端 Host 同步和传输的 55 项定向测试通过；SDK 构建及桌面类型检查通过。
